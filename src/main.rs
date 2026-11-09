use hidapi::{DeviceInfo, HidApi, HidDevice};
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;
use std::time::{Duration, Instant};

const VENDOR_ID: u16 = 0x041e;
const PRODUCT_ID: u16 = 0x3256;
const INTERFACE_NUMBER: i32 = 4;

/// One HID report ID byte plus the 64-byte report declared by the control
/// interface. Its descriptor contains no Report ID item, so hidapi requires the
/// leading `0x00` on write and omits it on read.
const REPORT_SIZE: usize = 65;
const PAYLOAD_SIZE: usize = 64;

const PAYLOAD_PREFIX: u8 = 0x5a;
const CMD_OUTPUT: u8 = 0x2c;
const CMD_LENGTH: u8 = 0x05;
const SELECTOR_WRITE: u8 = 0x00;
const SELECTOR_READ: u8 = 0x01;

/// Offsets inside a reply payload (`5a 2c 05 01 <mode>`), which hidapi hands
/// back without the report ID byte.
const REPLY_PREFIX: usize = 0;
const REPLY_COMMAND: usize = 1;
const REPLY_MODE: usize = 4;
const REPLY_MIN_LEN: usize = 5;

/// Direct Mode (`0x39`, parameter `0x05`). Verified on firmware bcdDevice 20.14:
/// `5a 39 03 00 05 <0|1>` switches it, `5a 39 03 01 05 00` reads it back as
/// `5a 39 05 01 <0x20 on | 0x00 off>`. A switch makes the G6 re-enumerate on
/// USB, so the write gets no reply and the state is read after it reconnects.
const CMD_DIRECT: u8 = 0x39;
const DIRECT_LENGTH: u8 = 0x03;
const DIRECT_PARAM: u8 = 0x05;
const DIRECT_REPLY_STATE: usize = 4;
const DIRECT_ON_FLAG: u8 = 0x20;
const DIRECT_DEADLINE: Duration = Duration::from_secs(15);
const DIRECT_RETRY: Duration = Duration::from_millis(500);

const READ_POLL_MS: i32 = 250;
const REPLY_DEADLINE: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Output {
    Speakers,
    Headphones,
}

impl Output {
    const fn device_mode(self) -> u8 {
        match self {
            Self::Speakers => 0x02,
            Self::Headphones => 0x04,
        }
    }

    const fn from_device_mode(mode: u8) -> Option<Self> {
        match mode {
            0x02 => Some(Self::Speakers),
            0x04 => Some(Self::Headphones),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Speakers => "speakers",
            Self::Headphones => "headphones",
        }
    }

    const fn opposite(self) -> Self {
        match self {
            Self::Speakers => Self::Headphones,
            Self::Headphones => Self::Speakers,
        }
    }
}

impl FromStr for Output {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "speaker" | "speakers" => Ok(Self::Speakers),
            "headphone" | "headphones" | "headset" => Ok(Self::Headphones),
            _ => Err(AppError::Usage(format!(
                "unknown output '{value}'; expected 'headphones' or 'speakers'"
            ))),
        }
    }
}

#[derive(Debug)]
enum AppError {
    DeviceNotFound,
    DeviceOpen(String),
    DeviceWrite(String),
    DeviceRead(String),
    InvalidWriteLength(usize),
    NoReply,
    UnknownDeviceMode(u8),
    NotApplied { requested: Output, actual: Output },
    DirectNotApplied(bool),
    State(String),
    Usage(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceNotFound => write!(
                formatter,
                "Sound BlasterX G6 control interface was not found (VID 041e, PID 3256, interface 4)"
            ),
            Self::DeviceOpen(message) => write!(
                formatter,
                "failed to open the Sound BlasterX G6 control interface: {message}. Check the udev rule and reconnect the device"
            ),
            Self::DeviceWrite(message) => {
                write!(formatter, "failed to send the output command: {message}")
            }
            Self::DeviceRead(message) => {
                write!(formatter, "failed to read the device reply: {message}")
            }
            Self::InvalidWriteLength(length) => write!(
                formatter,
                "the HID backend wrote {length} bytes instead of {REPORT_SIZE}"
            ),
            Self::NoReply => write!(
                formatter,
                "the device did not acknowledge the output command within {} ms",
                REPLY_DEADLINE.as_millis()
            ),
            Self::UnknownDeviceMode(mode) => write!(
                formatter,
                "the device reported an unknown output mode 0x{mode:02x}"
            ),
            Self::NotApplied { requested, actual } => write!(
                formatter,
                "the device rejected the command: requested {}, but it reports {}",
                requested.as_str(),
                actual.as_str()
            ),
            Self::DirectNotApplied(on) => write!(
                formatter,
                "Direct Mode did not turn {} within {} s",
                on_off(*on),
                DIRECT_DEADLINE.as_secs()
            ),
            Self::State(message) => write!(formatter, "failed to update local state: {message}"),
            Self::Usage(message) => write!(formatter, "{message}"),
        }
    }
}

impl Error for AppError {}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("g6-output: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), AppError> {
    let Some(command) = arguments.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };

    match command {
        "set" => {
            let output = arguments
                .get(1)
                .ok_or_else(|| {
                    AppError::Usage("usage: g6-output set <headphones|speakers>".into())
                })?
                .parse()?;
            set_output(output)?;
            println!("{}", output.as_str());
        }
        "headphones" | "headphone" => {
            set_output(Output::Headphones)?;
            println!("headphones");
        }
        "speakers" | "speaker" => {
            set_output(Output::Speakers)?;
            println!("speakers");
        }
        "toggle" => {
            let next = query_output()?.opposite();
            set_output(next)?;
            println!("{}", next.as_str());
        }
        "direct" => {
            let on = match arguments.get(1).map(String::as_str) {
                None => {
                    println!("{}", on_off(query_direct()?));
                    return Ok(());
                }
                Some("on") => true,
                Some("off") => false,
                Some("toggle") => !query_direct()?,
                Some(other) => {
                    return Err(AppError::Usage(format!(
                        "unknown Direct Mode value '{other}'; expected 'on', 'off' or 'toggle'"
                    )))
                }
            };
            set_direct(on)?;
            println!("{}", on_off(on));
        }
        "status" => println!("{}", status_line()),
        "doctor" => doctor()?,
        "--version" | "-V" | "version" => println!("g6-output {}", env!("CARGO_PKG_VERSION")),
        "--help" | "-h" | "help" => print_help(),
        other => {
            return Err(AppError::Usage(format!(
                "unknown command '{other}'. Run 'g6-output --help'"
            )))
        }
    }

    Ok(())
}

fn print_help() {
    println!(
        "g6-output {version}\n\
         Minimal Sound BlasterX G6 output switcher\n\n\
         USAGE:\n\
           g6-output set <headphones|speakers>\n\
           g6-output headphones\n\
           g6-output speakers\n\
           g6-output toggle\n\
           g6-output direct [on|off|toggle]\n\
           g6-output status\n\
           g6-output doctor\n",
        version = env!("CARGO_PKG_VERSION")
    );
}

/// `status` never fails: the GNOME indicator polls it at startup and only needs
/// a word it can map. The device is authoritative, the cache is a fallback for
/// when it is unplugged.
fn status_line() -> &'static str {
    match query_output() {
        Ok(output) => output.as_str(),
        Err(AppError::DeviceNotFound) => "disconnected",
        Err(_) => load_cached_output().map_or("unknown", Output::as_str),
    }
}

fn open_control_interface() -> Result<HidDevice, AppError> {
    let api = HidApi::new().map_err(|error| AppError::DeviceOpen(error.to_string()))?;
    let info = find_device(&api).ok_or(AppError::DeviceNotFound)?;
    info.open_device(&api)
        .map_err(|error| AppError::DeviceOpen(error.to_string()))
}

fn set_output(output: Output) -> Result<(), AppError> {
    let device = open_control_interface()?;

    write_packet(
        &device,
        &output_packet(SELECTOR_WRITE, output.device_mode()),
    )?;

    // The device answers a write with the read-form frame carrying the value it
    // actually applied, so this is a real confirmation rather than a guess.
    let actual = read_output_reply(&device)?;
    if actual != output {
        return Err(AppError::NotApplied {
            requested: output,
            actual,
        });
    }

    save_cached_output(output)?;
    Ok(())
}

fn query_output() -> Result<Output, AppError> {
    let device = open_control_interface()?;
    write_packet(&device, &output_packet(SELECTOR_READ, 0x00))?;
    read_output_reply(&device)
}

const fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

fn direct_packet(selector: u8, value: u8) -> [u8; REPORT_SIZE] {
    let mut packet = [0_u8; REPORT_SIZE];
    packet[1] = PAYLOAD_PREFIX;
    packet[2] = CMD_DIRECT;
    packet[3] = DIRECT_LENGTH;
    packet[4] = selector;
    packet[5] = DIRECT_PARAM;
    packet[6] = value;
    packet
}

fn read_direct_reply(device: &HidDevice) -> Result<bool, AppError> {
    let deadline = Instant::now() + REPLY_DEADLINE;
    let mut buffer = [0_u8; PAYLOAD_SIZE];
    while Instant::now() < deadline {
        let length = device
            .read_timeout(&mut buffer, READ_POLL_MS)
            .map_err(|error| AppError::DeviceRead(error.to_string()))?;
        // Selector 0x01 tells the read reply apart from other 0x39 frames.
        if length > DIRECT_REPLY_STATE
            && buffer[REPLY_PREFIX] == PAYLOAD_PREFIX
            && buffer[REPLY_COMMAND] == CMD_DIRECT
            && buffer[3] == SELECTOR_READ
        {
            return Ok(buffer[DIRECT_REPLY_STATE] & DIRECT_ON_FLAG != 0);
        }
    }
    Err(AppError::NoReply)
}

fn query_direct_on(device: &HidDevice) -> Result<bool, AppError> {
    write_packet(device, &direct_packet(SELECTOR_READ, 0x00))?;
    read_direct_reply(device)
}

fn query_direct() -> Result<bool, AppError> {
    query_direct_on(&open_control_interface()?)
}

fn set_direct(on: bool) -> Result<(), AppError> {
    let device = open_control_interface()?;
    if query_direct_on(&device)? == on {
        // Writing the current value would still reset the device and cut audio.
        return Ok(());
    }
    write_packet(&device, &direct_packet(SELECTOR_WRITE, u8::from(on)))?;
    drop(device);

    let deadline = Instant::now() + DIRECT_DEADLINE;
    while Instant::now() < deadline {
        std::thread::sleep(DIRECT_RETRY);
        // Errors are expected while the G6 is off the bus; keep retrying.
        if query_direct().is_ok_and(|state| state == on) {
            return Ok(());
        }
    }
    Err(AppError::DirectNotApplied(on))
}

fn find_device(api: &HidApi) -> Option<&DeviceInfo> {
    api.device_list().find(|device| {
        device.vendor_id() == VENDOR_ID
            && device.product_id() == PRODUCT_ID
            && device.interface_number() == INTERFACE_NUMBER
    })
}

fn write_packet(device: &HidDevice, packet: &[u8; REPORT_SIZE]) -> Result<(), AppError> {
    let written = device
        .write(packet)
        .map_err(|error| AppError::DeviceWrite(error.to_string()))?;

    if written != REPORT_SIZE {
        return Err(AppError::InvalidWriteLength(written));
    }

    Ok(())
}

fn read_output_reply(device: &HidDevice) -> Result<Output, AppError> {
    let deadline = Instant::now() + REPLY_DEADLINE;
    let mut buffer = [0_u8; PAYLOAD_SIZE];

    while Instant::now() < deadline {
        let length = device
            .read_timeout(&mut buffer, READ_POLL_MS)
            .map_err(|error| AppError::DeviceRead(error.to_string()))?;

        if length == 0 {
            continue;
        }

        // The G6 also emits unsolicited event frames (for example `5a 6e ...`
        // when the output changes), so match on the command byte rather than
        // trusting whichever report arrives first.
        if length >= REPLY_MIN_LEN
            && buffer[REPLY_PREFIX] == PAYLOAD_PREFIX
            && buffer[REPLY_COMMAND] == CMD_OUTPUT
        {
            let mode = buffer[REPLY_MODE];
            return Output::from_device_mode(mode).ok_or(AppError::UnknownDeviceMode(mode));
        }
    }

    Err(AppError::NoReply)
}

fn output_packet(selector: u8, mode: u8) -> [u8; REPORT_SIZE] {
    let mut packet = [0_u8; REPORT_SIZE];

    // packet[0] is HID report ID 0. The G6 payload starts at packet[1].
    packet[1] = PAYLOAD_PREFIX;
    packet[2] = CMD_OUTPUT;
    packet[3] = CMD_LENGTH;
    packet[4] = selector;
    packet[5] = mode;

    packet
}

fn doctor() -> Result<(), AppError> {
    let api = HidApi::new().map_err(|error| AppError::DeviceOpen(error.to_string()))?;

    println!("Looking for Sound BlasterX G6:");
    println!("  vendor:    041e");
    println!("  product:   3256");
    println!("  interface: 4");

    let Some(info) = find_device(&api) else {
        println!("  result:    not found");
        list_related_interfaces(&api);
        return Err(AppError::DeviceNotFound);
    };

    println!("  path:      {}", info.path().to_string_lossy());
    println!("  result:    found");

    let device = info
        .open_device(&api)
        .map_err(|error| AppError::DeviceOpen(error.to_string()))?;

    println!("  access:    OK");

    write_packet(&device, &output_packet(SELECTOR_READ, 0x00))?;
    match read_output_reply(&device) {
        Ok(output) => println!("  device:    {}", output.as_str()),
        Err(error) => println!("  device:    query failed ({error})"),
    }

    match query_direct_on(&device) {
        Ok(on) => println!("  direct:    {}", on_off(on)),
        Err(error) => println!("  direct:    query failed ({error})"),
    }

    println!(
        "  cached:    {}",
        load_cached_output().map_or("unknown", Output::as_str)
    );
    Ok(())
}

/// When the expected interface is missing, show the other interfaces the device
/// exposes so a mismatched interface number is obvious instead of silent.
fn list_related_interfaces(api: &HidApi) {
    let candidates: Vec<&DeviceInfo> = api
        .device_list()
        .filter(|device| device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID)
        .collect();

    if candidates.is_empty() {
        println!("  hint:      no 041e:3256 HID interface is visible at all");
        return;
    }

    println!("  hint:      the device is present, but not on interface 4:");
    for candidate in candidates {
        println!(
            "             interface {} at {}",
            candidate.interface_number(),
            candidate.path().to_string_lossy()
        );
    }
}

fn state_path() -> Result<PathBuf, AppError> {
    let base = match env::var_os("XDG_STATE_HOME") {
        Some(path) => PathBuf::from(path),
        None => {
            let home =
                env::var_os("HOME").ok_or_else(|| AppError::State("HOME is not set".into()))?;
            PathBuf::from(home).join(".local/state")
        }
    };

    Ok(base.join("g6-output/output"))
}

fn save_cached_output(output: Output) -> Result<(), AppError> {
    let path = state_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::State("invalid state path".into()))?;

    fs::create_dir_all(parent).map_err(|error| AppError::State(error.to_string()))?;
    fs::write(path, format!("{}\n", output.as_str()))
        .map_err(|error| AppError::State(error.to_string()))
}

fn load_cached_output() -> Option<Output> {
    let path = state_path().ok()?;
    let value = fs::read_to_string(path).ok()?;
    value.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_output_aliases() {
        assert_eq!("headphones".parse::<Output>().unwrap(), Output::Headphones);
        assert_eq!("headphone".parse::<Output>().unwrap(), Output::Headphones);
        assert_eq!("headset".parse::<Output>().unwrap(), Output::Headphones);
        assert_eq!("speakers".parse::<Output>().unwrap(), Output::Speakers);
        assert_eq!("speaker".parse::<Output>().unwrap(), Output::Speakers);
    }

    #[test]
    fn parses_output_aliases_case_insensitively() {
        assert_eq!("HeadPhones".parse::<Output>().unwrap(), Output::Headphones);
        assert_eq!("SPEAKERS".parse::<Output>().unwrap(), Output::Speakers);
    }

    #[test]
    fn rejects_unknown_output_names() {
        for value in ["", "optical", "spdif", "head phones"] {
            assert!(value.parse::<Output>().is_err(), "{value} should not parse");
        }
    }

    #[test]
    fn builds_headphones_packet() {
        let packet = output_packet(SELECTOR_WRITE, Output::Headphones.device_mode());
        assert_eq!(
            &packet[..9],
            &[0x00, 0x5a, 0x2c, 0x05, 0x00, 0x04, 0x00, 0x00, 0x00]
        );
        assert!(packet[9..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn builds_speakers_packet() {
        let packet = output_packet(SELECTOR_WRITE, Output::Speakers.device_mode());
        assert_eq!(
            &packet[..9],
            &[0x00, 0x5a, 0x2c, 0x05, 0x00, 0x02, 0x00, 0x00, 0x00]
        );
        assert!(packet[9..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn builds_query_packet() {
        let packet = output_packet(SELECTOR_READ, 0x00);
        assert_eq!(
            &packet[..9],
            &[0x00, 0x5a, 0x2c, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00]
        );
        assert!(packet[9..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn builds_direct_packets() {
        assert_eq!(
            &direct_packet(SELECTOR_WRITE, 1)[..8],
            &[0x00, 0x5a, 0x39, 0x03, 0x00, 0x05, 0x01, 0x00]
        );
        assert_eq!(
            &direct_packet(SELECTOR_READ, 0)[..8],
            &[0x00, 0x5a, 0x39, 0x03, 0x01, 0x05, 0x00, 0x00]
        );
    }

    #[test]
    fn device_mode_round_trips() {
        for output in [Output::Headphones, Output::Speakers] {
            assert_eq!(Output::from_device_mode(output.device_mode()), Some(output));
        }
    }

    #[test]
    fn rejects_unknown_device_modes() {
        for mode in [0x00, 0x01, 0x03, 0x05, 0xff] {
            assert_eq!(Output::from_device_mode(mode), None, "mode {mode:#04x}");
        }
    }

    #[test]
    fn opposite_is_an_involution() {
        for output in [Output::Headphones, Output::Speakers] {
            assert_eq!(output.opposite().opposite(), output);
            assert_ne!(output.opposite(), output);
        }
    }
}
