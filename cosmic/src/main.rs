use cosmic::iced::platform_specific::shell::wayland::commands::popup::destroy_popup;
use cosmic::iced::{Alignment, Length, Subscription, window};
use cosmic::{Element, Task, app, widget};
use std::{path::PathBuf, sync::LazyLock, time::Duration};

static AUTOSIZE: LazyLock<widget::Id> = LazyLock::new(widget::Id::unique);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Output {
    Headphones,
    Speakers,
    Disconnected,
    Unknown,
}

impl Output {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "headphones" => Ok(Self::Headphones),
            "speakers" => Ok(Self::Speakers),
            "disconnected" => Ok(Self::Disconnected),
            other => Err(format!("Неизвестный ответ G6: {other}")),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Headphones => "Наушники",
            Self::Speakers => "Колонки",
            Self::Disconnected => "G6 не подключён",
            Self::Unknown => "Выход неизвестен",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Headphones => "audio-headphones-symbolic",
            Self::Speakers => "audio-speakers-symbolic",
            _ => "audio-card-symbolic",
        }
    }
    fn argument(self) -> &'static str {
        match self {
            Self::Headphones => "headphones",
            Self::Speakers => "speakers",
            _ => "",
        }
    }
}

fn local_file(path: &str) -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(path)
}

type Report = (Output, Option<bool>);

fn parse_doctor(stdout: &str) -> Result<Report, String> {
    if stdout.lines().any(|line| {
        line.trim()
            .strip_prefix("result:")
            .is_some_and(|value| value.trim() == "not found")
    }) {
        return Ok((Output::Disconnected, None));
    }
    let device = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("device:").map(str::trim))
        .ok_or_else(|| "Не удалось прочитать выход G6. Проверь USB и доступ к HID.".to_owned())?;
    let direct = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("direct:").map(str::trim))
        .and_then(|value| match value {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        });
    Ok((Output::parse(device)?, direct))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Operation {
    Read,
    Toggle,
    Set(Output),
    Direct(bool),
}

async fn run(operation: Operation) -> Result<Report, String> {
    let mut command = tokio::process::Command::new(local_file(".local/bin/g6-output"));
    // A Direct Mode switch re-enumerates the G6 on USB, which takes ~2.5 s.
    let deadline = Duration::from_secs(if matches!(operation, Operation::Direct(_)) {
        20
    } else {
        3
    });
    command.kill_on_drop(true);
    match operation {
        Operation::Read => {
            command.arg("doctor");
        }
        Operation::Toggle => {
            command.arg("toggle");
        }
        Operation::Set(output) => {
            command.args(["set", output.argument()]);
        }
        Operation::Direct(on) => {
            command.args(["direct", if on { "on" } else { "off" }]);
        }
    }
    let result = tokio::time::timeout(deadline, command.output())
        .await
        .map_err(|_| {
            format!(
                "G6 не ответил за {} с. Проверь подключение USB.",
                deadline.as_secs()
            )
        })?
        .map_err(|error| format!("Не удалось запустить g6-output: {error}"))?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    if matches!(operation, Operation::Read) {
        // `status` can return its cache after a failed HID read; doctor distinguishes a real reply.
        let parsed = parse_doctor(&stdout);
        if result.status.success() || matches!(parsed, Ok((Output::Disconnected, _))) {
            return parsed;
        }
    }
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }
    if matches!(operation, Operation::Direct(_)) {
        return Box::pin(run(Operation::Read)).await;
    }
    Ok((Output::parse(&stdout)?, None))
}

struct App {
    core: app::Core,
    output: Output,
    direct: Option<bool>,
    busy: Option<Operation>,
    pending: Option<Operation>,
    error: Option<String>,
    popup: Option<window::Id>,
}

#[derive(Debug, Clone)]
enum Message {
    Poll,
    Switch(Operation),
    Finished(Operation, Result<Report, String>),
    TogglePopup,
    CloseRequested(window::Id),
    MenuKey(window::Id),
}

impl App {
    fn switching(&self) -> bool {
        matches!(
            self.busy,
            Some(Operation::Toggle | Operation::Set(_) | Operation::Direct(_))
        ) || self.pending.is_some()
    }
    fn execute(&mut self, operation: Operation) -> app::Task<Message> {
        if let Some(active) = self.busy {
            if matches!(active, Operation::Read) && !matches!(operation, Operation::Read) {
                self.pending = Some(operation);
            }
            return Task::none();
        }
        self.busy = Some(operation);
        Task::perform(run(operation), move |result| {
            cosmic::Action::App(Message::Finished(operation, result))
        })
    }
    fn popup_task(&mut self) -> app::Task<Message> {
        cosmic::surface::surface_task(cosmic::surface::action::app_popup(
            |_| Default::default(),
            |app: &mut Self| {
                let id = window::Id::unique();
                app.popup = Some(id);
                app.core.applet.get_popup_settings(
                    app.core.main_window_id().unwrap(),
                    id,
                    None,
                    None,
                    None,
                )
            },
            None,
        ))
    }
}

impl cosmic::Application for App {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "io.github.netherguy4.G6Output";
    fn core(&self) -> &app::Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }
    fn init(core: app::Core, _: ()) -> (Self, app::Task<Message>) {
        let mut app = Self {
            core,
            output: Output::Unknown,
            direct: None,
            busy: None,
            pending: None,
            error: None,
            popup: None,
        };
        let task = app.execute(Operation::Read);
        (app, task)
    }
    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
    fn subscription(&self) -> Subscription<Message> {
        // ponytail: hot-plug updates poll every 5 s; use udev events if immediate updates are required.
        Subscription::batch([
            cosmic::iced::time::every(Duration::from_secs(5)).map(|_| Message::Poll),
            cosmic::iced::event::listen_with(|event, _, id| {
                use cosmic::iced::keyboard::{Event, Key, key::Named};
                match event {
                    cosmic::iced::Event::Keyboard(Event::KeyPressed {
                        key: Key::Named(key),
                        modifiers,
                        ..
                    }) if key == Named::ContextMenu || (key == Named::F10 && modifiers.shift()) => {
                        Some(Message::MenuKey(id))
                    }
                    _ => None,
                }
            }),
        ])
    }
    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Poll => return self.execute(Operation::Read),
            Message::Switch(operation) => {
                if matches!(operation, Operation::Toggle)
                    && let Some(id) = self.popup.take()
                {
                    return Task::batch([destroy_popup(id), self.execute(operation)]);
                }
                return self.execute(operation);
            }
            Message::Finished(operation, result) => {
                self.busy = None;
                match result {
                    Ok((output, direct)) => {
                        self.output = output;
                        // Output switches do not read Direct Mode; keep the last known state.
                        if matches!(operation, Operation::Read | Operation::Direct(_)) {
                            self.direct = direct;
                        }
                        self.error = None;
                    }
                    Err(error) => {
                        self.output = Output::Unknown;
                        self.direct = None;
                        self.error = Some(error);
                        if !matches!(operation, Operation::Read) && self.popup.is_none() {
                            return self.popup_task();
                        }
                    }
                }
                if let Some(pending) = self.pending.take() {
                    return self.execute(pending);
                }
            }
            Message::MenuKey(id) => {
                if self.core.main_window_id() == Some(id) {
                    return self.update(Message::TogglePopup);
                }
            }
            Message::TogglePopup => {
                if let Some(id) = self.popup.take() {
                    return destroy_popup(id);
                }
                let popup = self.popup_task();
                let refresh = self.execute(Operation::Read);
                return Task::batch([popup, refresh]);
            }
            Message::CloseRequested(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }
        }
        Task::none()
    }
    fn view(&self) -> Element<'_, Message> {
        let content = widget::row(vec![])
            .spacing(4)
            .align_y(Alignment::Center)
            .push(widget::icon::from_name(self.output.icon()).size(20))
            .push(widget::text(if self.error.is_some() { "G6 !" } else { "G6" }).size(14));
        let button = widget::button::custom(content)
            .padding([0, self.core.applet.suggested_padding(true).0])
            .on_press_maybe(
                (!self.switching() && self.output != Output::Disconnected)
                    .then_some(Message::Switch(Operation::Toggle)),
            )
            .description(format!(
                "Sound BlasterX G6: {}. Левый клик переключает выход, правый открывает меню.",
                self.output.label()
            ))
            .class(cosmic::theme::Button::AppletIcon);
        widget::autosize::autosize(
            cosmic::iced::widget::mouse_area(button).on_right_release(Message::TogglePopup),
            AUTOSIZE.clone(),
        )
        .into()
    }
    fn view_window(&self, _: window::Id) -> Element<'_, Message> {
        let photo = widget::icon::from_path(local_file(
            ".local/share/icons/hicolor/256x256/apps/sound-blasterx-g6.png",
        ));
        let mut content = widget::column(vec![])
            .spacing(12)
            .padding(16)
            .width(Length::Fill)
            .push(
                widget::row(vec![])
                    .spacing(12)
                    .align_y(Alignment::Center)
                    .push(widget::icon(photo).size(64))
                    .push(
                        widget::column(vec![])
                            .spacing(4)
                            .push(widget::text::title4("Sound BlasterX G6"))
                            .push(widget::text(if self.switching() {
                                "Переключаю…"
                            } else {
                                self.output.label()
                            })),
                    ),
            )
            .push(widget::divider::horizontal::default());
        for output in [Output::Headphones, Output::Speakers] {
            let mut row = widget::row(vec![])
                .spacing(12)
                .align_y(Alignment::Center)
                .push(widget::icon::from_name(output.icon()).size(20))
                .push(widget::text(output.label()))
                .push(widget::space::horizontal().width(Length::Fill));
            if output == self.output {
                row = row.push(widget::icon::from_name("object-select-symbolic").size(16));
            }
            content = content.push(
                widget::button::custom(row)
                    .width(Length::Fill)
                    .padding(10)
                    .on_press_maybe(
                        (!self.switching() && self.output != Output::Disconnected)
                            .then_some(Message::Switch(Operation::Set(output))),
                    )
                    .description(output.label())
                    .class(cosmic::theme::Button::Text),
            );
        }
        if let Some(direct) = self.direct {
            let toggler = widget::toggler(direct)
                .label("Direct Mode".to_owned())
                .width(Length::Fill)
                .on_toggle_maybe(
                    (!self.switching()).then_some(|on| Message::Switch(Operation::Direct(on))),
                );
            content = content
                .push(widget::divider::horizontal::default())
                .push(widget::container(toggler).padding([0, 10]));
        }
        if let Some(error) = &self.error {
            content = content.push(widget::text(format!(
                "Не удалось прочитать или переключить выход: {error}"
            )));
        }
        self.core
            .applet
            .popup_container(widget::container(content).width(360))
            .into()
    }
    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::CloseRequested(id))
    }
}

fn main() -> cosmic::iced::Result {
    cosmic::applet::run::<App>(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::Application;
    #[test]
    fn trusts_device_reply_instead_of_cached_output() {
        assert_eq!(
            parse_doctor("  device:    speakers\n  cached:    headphones\n"),
            Ok((Output::Speakers, None))
        );
        assert_eq!(
            parse_doctor("  device:    headphones\n  direct:    on\n"),
            Ok((Output::Headphones, Some(true)))
        );
        assert_eq!(
            parse_doctor("  device:    headphones\n  direct:    query failed (x)\n"),
            Ok((Output::Headphones, None))
        );
        assert!(
            parse_doctor("  device:    query failed (timeout)\n  cached:    headphones\n").is_err()
        );
        assert_eq!(
            parse_doctor("  result:    not found\n"),
            Ok((Output::Disconnected, None))
        );
        assert!(Output::parse("headphones\nunknown").is_err());
    }
    #[test]
    fn background_read_stays_quiet_and_queues_user_choice() {
        let mut app = App {
            core: app::Core::default(),
            output: Output::Headphones,
            direct: Some(true),
            busy: None,
            pending: None,
            error: None,
            popup: None,
        };
        let _ = app.execute(Operation::Read);
        assert!(!app.switching());
        let _ = app.execute(Operation::Set(Output::Speakers));
        assert!(app.switching());
        assert_eq!(app.busy, Some(Operation::Read));
        let _ = app.update(Message::Finished(
            Operation::Read,
            Ok((Output::Headphones, Some(true))),
        ));
        assert_eq!(app.busy, Some(Operation::Set(Output::Speakers)));
        assert!(app.pending.is_none());
        let _ = app.update(Message::Finished(
            Operation::Set(Output::Speakers),
            Ok((Output::Speakers, None)),
        ));
        assert!(!app.switching());
        assert_eq!(app.output, Output::Speakers);
        assert_eq!(
            app.direct,
            Some(true),
            "output switch keeps Direct Mode state"
        );
    }
}
