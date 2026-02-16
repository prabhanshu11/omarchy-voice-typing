# Python Version Upgrade Guide (Arch Linux)

Arch Linux upgrades Python in-place (e.g., 3.13 -> 3.14). This breaks hyprwhspr because:

1. System packages (e.g., `python-sounddevice`) install to `/usr/lib/python3.XX/site-packages/`
2. After upgrade, the new Python doesn't look in the old version's site-packages
3. The `python3.XX` binary for the old version is removed

## Symptoms

The hyprwhspr service crash-loops with:
```
ModuleNotFoundError: No module named 'sounddevice'
```

Check with: `journalctl --user -u hyprwhspr -n 20`

## Fix Procedure

### 1. Recreate the venv

```bash
rm -rf ~/.local/share/hyprwhspr/venv
python3 -m venv ~/.local/share/hyprwhspr/venv
```

### 2. Install missing packages in the venv

```bash
~/.local/share/hyprwhspr/venv/bin/pip install sounddevice websocket-client
```

To find what else might be missing, test imports:
```bash
PYTHONPATH=/home/prabhanshu/.local/lib/hyprwhspr-patch:/usr/lib/hyprwhspr/lib:/home/prabhanshu/.local/share/hyprwhspr/venv/lib/pythonX.YY/site-packages \
  /usr/bin/python -c "import sounddevice; import websocket; print('OK')"
```

### 3. Update the systemd override PYTHONPATH

Edit `~/.config/systemd/user/hyprwhspr.service.d/override.conf` and update the `PYTHONPATH` to include the new venv's site-packages path:

```
Environment=PYTHONPATH=/home/prabhanshu/.local/lib/hyprwhspr-patch:/usr/lib/hyprwhspr/lib:/home/prabhanshu/.local/share/hyprwhspr/venv/lib/pythonX.YY/site-packages
```

Replace `pythonX.YY` with the new Python version (e.g., `python3.14`).

### 4. Restart

```bash
systemctl --user daemon-reload
systemctl --user restart hyprwhspr
systemctl --user status hyprwhspr
```

## Why not `pip install --user` or `sudo pip install`?

- `pip install --user` is blocked by PEP 668 (externally-managed-environment) on Arch
- `sudo pip install` also blocked by PEP 668 and risks breaking system packages
- The venv approach is the correct PEP 668 solution and survives future upgrades (just recreate the venv)

## Affected packages (as of 2026-02-16)

| Package | System pkg | Venv install |
|---------|-----------|--------------|
| sounddevice | `python-sounddevice` (3.13 only) | `pip install sounddevice` |
| websocket-client | `python-websocket-client` (3.13 only) | `pip install websocket-client` |

Other deps (`websockets`, `cffi`, `requests`, `numpy`, `gi`) were already available for 3.14.
