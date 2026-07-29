import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import GUdev from 'gi://GUdev';
import St from 'gi://St';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

const OUTPUTS = {
    headphones: {
        label: 'Headphones',
        icon: 'audio-headphones-symbolic',
    },
    speakers: {
        label: 'Speakers',
        icon: 'audio-volume-high-symbolic',
    },
};

const FALLBACK_ICON = 'audio-card-symbolic';

const G6_VENDOR_ID = '041e';

// Coalesces the burst of uevents a multi-interface device emits, and gives
// systemd's uaccess rule time to apply the ACL before the first query.
const UEVENT_DEBOUNCE_MS = 300;

// A freshly plugged device is not always enumerable the instant udev announces
// it, so a connect gets a couple of extra chances. A removal is final and takes
// effect immediately.
const UEVENT_RETRY_MS = 400;
const CONNECT_RETRIES = 2;

const G6Indicator = GObject.registerClass(
class G6Indicator extends PanelMenu.Button {
    _init(binaryPath) {
        super._init(0.0, 'Sound Blaster G6 Output');

        this._binaryPath = binaryPath;
        this._cancellable = new Gio.Cancellable();
        this._refreshTimer = 0;
        this._busy = false;
        this._output = null;
        this._usingGestures = false;

        this._setupClickHandling();

        this._icon = new St.Icon({
            icon_name: FALLBACK_ICON,
            style_class: 'system-status-icon',
        });
        this.add_child(this._icon);

        this._headphonesItem = this._createOutputItem('headphones');
        this._speakersItem = this._createOutputItem('speakers');

        this.menu.addMenuItem(this._headphonesItem);
        this.menu.addMenuItem(this._speakersItem);

        // Hidden until the device is known to be present, so the panel stays
        // clean when the G6 is unplugged.
        this._setVisible(false);

        // Opening the menu re-reads the device, so a switch made by other
        // software shows up without waiting for a uevent.
        this._openStateId = this.menu.connect('open-state-changed', (menu, isOpen) => {
            if (isOpen && !this._busy)
                this._refreshFromDevice();
        });

        this._udevClient = new GUdev.Client({subsystems: ['hidraw']});
        this._ueventId = this._udevClient.connect('uevent',
            (client, action, device) => this._onUevent(action, device));

        this._refreshFromDevice();
    }

    _createOutputItem(output) {
        const config = OUTPUTS[output];
        const item = new PopupMenu.PopupImageMenuItem(config.label, config.icon);
        item.connect('activate', () => this._setOutput(output));
        return item;
    }

    // Left click switches straight away; the menu, which only offers the two
    // explicit choices, moves to the right button.
    //
    // GNOME 50 routes panel clicks through Clutter.ClickGesture, so overriding
    // vfunc_event there has no effect at all — the inherited gesture claims the
    // press first. Older versions still use vfunc_event, hence the two paths.
    _setupClickHandling() {
        if (!this._clickGesture || !Clutter.ClickGesture)
            return;

        // Restrict the inherited menu gesture to the right button...
        this._clickGesture.set_required_button(Clutter.BUTTON_SECONDARY);

        // ...and give the left one a gesture of its own. Two click gestures on
        // one actor, separated by required_button, is the shell's own pattern
        // (see backgroundMenu.js).
        this._toggleGesture = new Clutter.ClickGesture({
            required_button: Clutter.BUTTON_PRIMARY,
            recognize_on_press: true,
        });
        this._toggleGesture.connect('recognize', () => this._toggleOutput());
        this.add_action(this._toggleGesture);

        this._usingGestures = true;
    }

    vfunc_event(event) {
        if (this._usingGestures)
            return super.vfunc_event(event);

        if (event.type() === Clutter.EventType.BUTTON_PRESS) {
            switch (event.get_button()) {
            case Clutter.BUTTON_PRIMARY:
                this._toggleOutput();
                return Clutter.EVENT_STOP;
            case Clutter.BUTTON_SECONDARY:
                this.menu.toggle();
                return Clutter.EVENT_STOP;
            default:
                break;
            }
        }

        // Touch has no secondary button, so keep the menu reachable there.
        if (event.type() === Clutter.EventType.TOUCH_BEGIN) {
            this.menu.toggle();
            return Clutter.EVENT_STOP;
        }

        return super.vfunc_event(event);
    }

    _setVisible(visible) {
        this.visible = visible;
        // PanelMenu.ButtonBox wraps the actor in a container; keep both in sync
        // so hiding works whether or not the version binds them together.
        if (this.container)
            this.container.visible = visible;
    }

    _onUevent(action, device) {
        // Removals can arrive with a stripped property set, so any hidraw
        // removal is worth re-checking; adds and changes carry the vendor and
        // are filtered on it to avoid reacting to every other HID device.
        if (action !== 'remove' && device.get_property('ID_VENDOR_ID') !== G6_VENDOR_ID)
            return;

        this._scheduleRefresh(action === 'remove' ? 0 : CONNECT_RETRIES);
    }

    _scheduleRefresh(retries = 0, delay = UEVENT_DEBOUNCE_MS) {
        if (this._refreshTimer)
            GLib.Source.remove(this._refreshTimer);

        this._refreshTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, delay, () => {
            this._refreshTimer = 0;
            this._refreshFromDevice(retries);
            return GLib.SOURCE_REMOVE;
        });
    }

    _refreshFromDevice(retries = 0) {
        this._run(['status'], (stdout) => {
            const status = stdout.trim();

            if (status === 'disconnected' && retries > 0) {
                this._scheduleRefresh(retries - 1, UEVENT_RETRY_MS);
                return;
            }

            this._applyStatus(status);
        }, false);
    }

    // `toggle` reads the device before switching, so it never acts on a stale
    // guess even if something else changed the output.
    _toggleOutput() {
        if (this._busy)
            return;

        // Keep the two interactions from overlapping mid-animation.
        this.menu.close();

        this._busy = true;

        this._run(['toggle'], (stdout) => {
            this._applyStatus(stdout.trim());
            this._busy = false;
        }, true, () => {
            this._busy = false;
            this._refreshFromDevice();
        });
    }

    _setOutput(output) {
        if (this._busy)
            return;

        this._busy = true;

        this._run(['set', output], () => {
            this._applyStatus(output);
            this._busy = false;
        }, true, () => {
            this._busy = false;
            this._refreshFromDevice();
        });
    }

    _run(args, onSuccess, showError, onFailure = null) {
        if (!this._cancellable)
            return;

        let process;
        try {
            process = Gio.Subprocess.new(
                [this._binaryPath, ...args],
                Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE
            );
        } catch (error) {
            if (showError)
                Main.notifyError('Sound Blaster G6', error.message);
            onFailure?.();
            return;
        }

        process.communicate_utf8_async(null, this._cancellable, (source, result) => {
            // The indicator may have been destroyed while the command ran;
            // touching its actors after that would hit deallocated objects.
            if (!this._cancellable || this._cancellable.is_cancelled())
                return;

            try {
                const [, stdout, stderr] = source.communicate_utf8_finish(result);
                if (!source.get_successful()) {
                    throw new Error(stderr.trim() || `g6-output exited with status ${source.get_exit_status()}`);
                }
                onSuccess(stdout ?? '');
            } catch (error) {
                if (error.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    return;
                if (showError)
                    Main.notifyError('Sound Blaster G6', error.message);
                onFailure?.();
            }
        });
    }

    // `status` reports `headphones`, `speakers`, `disconnected` or `unknown`.
    // Success is conveyed by the panel icon alone; only failures interrupt.
    _applyStatus(status) {
        if (status === 'disconnected') {
            this._output = null;
            this._icon.icon_name = FALLBACK_ICON;
            this.menu.close();
            this._setVisible(false);
            return;
        }

        this._setVisible(true);
        this._output = Object.hasOwn(OUTPUTS, status) ? status : null;
        this._icon.icon_name = this._output ? OUTPUTS[this._output].icon : FALLBACK_ICON;
    }

    destroy() {
        if (this._refreshTimer) {
            GLib.Source.remove(this._refreshTimer);
            this._refreshTimer = 0;
        }

        if (this._ueventId) {
            this._udevClient.disconnect(this._ueventId);
            this._ueventId = 0;
        }
        this._udevClient = null;

        if (this._openStateId) {
            this.menu.disconnect(this._openStateId);
            this._openStateId = 0;
        }

        // The action itself is owned by the actor and goes with it.
        this._toggleGesture = null;

        this._cancellable?.cancel();
        this._cancellable = null;

        super.destroy();
    }
});

export default class SoundBlasterG6OutputExtension extends Extension {
    enable() {
        const binaryPath = this._resolveBinary();
        if (!binaryPath) {
            Main.notifyError(
                'Sound Blaster G6',
                'g6-output was not found in ~/.local/bin or on PATH. Run install.sh again.'
            );
            return;
        }

        this._indicator = new G6Indicator(binaryPath);
        Main.panel.addToStatusArea(this.uuid, this._indicator);
    }

    // install.sh puts the CLI in ~/.local/bin, but a packaged or system-wide
    // install may put it elsewhere, so fall back to PATH rather than failing
    // silently.
    _resolveBinary() {
        const localPath = GLib.build_filenamev([
            GLib.get_home_dir(),
            '.local',
            'bin',
            'g6-output',
        ]);

        if (GLib.file_test(localPath, GLib.FileTest.IS_EXECUTABLE))
            return localPath;

        return GLib.find_program_in_path('g6-output');
    }

    disable() {
        this._indicator?.destroy();
        this._indicator = null;
    }
}
