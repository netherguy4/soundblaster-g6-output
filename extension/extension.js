import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
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
        icon: 'audio-speakers-symbolic',
    },
};

const DEFAULT_ICON = 'audio-card-symbolic';

const G6Indicator = GObject.registerClass(
class G6Indicator extends PanelMenu.Button {
    _init(binaryPath) {
        super._init(0.0, 'Sound Blaster G6 Output');

        this._binaryPath = binaryPath;
        this._cancellable = new Gio.Cancellable();
        this._busy = false;
        this._output = null;
        this._detail = 'unknown';

        this._icon = new St.Icon({
            icon_name: DEFAULT_ICON,
            style_class: 'system-status-icon',
        });
        this.add_child(this._icon);

        this._statusItem = new PopupMenu.PopupMenuItem('Output: unknown', {
            reactive: false,
        });
        this.menu.addMenuItem(this._statusItem);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._headphonesItem = this._createOutputItem('headphones');
        this._speakersItem = this._createOutputItem('speakers');

        this.menu.addMenuItem(this._headphonesItem);
        this.menu.addMenuItem(this._speakersItem);

        // Refresh whenever the menu opens, so a switch made outside this
        // extension is picked up instead of showing a stale label.
        this._openStateId = this.menu.connect('open-state-changed', (menu, isOpen) => {
            if (isOpen && !this._busy)
                this._refreshFromDevice();
        });

        this._refreshFromDevice();
    }

    _createOutputItem(output) {
        const config = OUTPUTS[output];
        const item = new PopupMenu.PopupImageMenuItem(config.label, config.icon);
        item.connect('activate', () => this._setOutput(output));
        return item;
    }

    _refreshFromDevice() {
        this._run(['status'], (stdout) => {
            this._applyStatus(stdout.trim(), false);
        }, false);
    }

    _setOutput(output) {
        if (this._busy)
            return;

        this._busy = true;
        this._setItemsSensitive(false);
        this._statusItem.label.text = `Switching to ${OUTPUTS[output].label}…`;

        this._run(['set', output], () => {
            this._applyStatus(output, true);
            this._busy = false;
            this._setItemsSensitive(true);
        }, true, () => {
            this._busy = false;
            this._setItemsSensitive(true);
            this._refreshStatusLabel();
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

    // `status` also reports `disconnected` and `unknown`, which have no icon of
    // their own but must still produce a truthful label.
    _applyStatus(status, notify) {
        this._output = Object.hasOwn(OUTPUTS, status) ? status : null;
        this._detail = status;
        this._icon.icon_name = this._output ? OUTPUTS[this._output].icon : DEFAULT_ICON;
        this._refreshStatusLabel();

        if (notify && this._output)
            Main.notify('Sound Blaster G6', `Output: ${OUTPUTS[this._output].label}`);
    }

    _refreshStatusLabel() {
        if (this._output) {
            this._statusItem.label.text = `Output: ${OUTPUTS[this._output].label}`;
            return;
        }

        this._statusItem.label.text = this._detail === 'disconnected'
            ? 'Device disconnected'
            : 'Output: unknown';
    }

    _setItemsSensitive(sensitive) {
        this._headphonesItem.setSensitive(sensitive);
        this._speakersItem.setSensitive(sensitive);
    }

    destroy() {
        if (this._openStateId) {
            this.menu.disconnect(this._openStateId);
            this._openStateId = null;
        }

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
