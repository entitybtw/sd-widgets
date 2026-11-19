# API

All routes are JSON over HTTP on `http://<host>:47830` (default). The port is
configurable in the daemon.

Every route except `GET /api/v1/ping` and `POST /api/v1/pair` requires:

```
Authorization: Bearer <token>
```

`sdw-deck` obtains the token once by pairing with the password shown in the daemon
and stores it per host.

## Discovery

`47832/udp` — broadcast a JSON `DiscoverRequest` and each daemon answers with
`DiscoveryResponse` (daemon id, name, version, address, port, `setup_done`).

## Pairing

### `GET /api/v1/ping`

Public. Returns `Ping`: name, version, protocol, daemon id, `setup_done`,
`auth_required`.

### `POST /api/v1/pair`

```json
{ "password": "…", "client_name": "Steam Deck", "client_id": "…" }
```

Returns `PairResponse`:

```json
{ "token": "…", "daemon_name": "desktop", "daemon_id": "…", "version": "0.1.0", "api_port": 47830 }
```

Wrong password: `401`.

## Status and config

| Method | Path | Body / result |
| --- | --- | --- |
| `GET` | `/api/v1/status` | `DaemonStatus`: host, platform, VIIPER state, pads, monitor state, Home Assistant summary, setup flags. |
| `GET` | `/api/v1/config` | Redacted daemon config. |
| `PATCH` | `/api/v1/config` | Partial config update. |
| `GET` | `/api/v1/logs` | Recent log lines. |

## Gamepads

| Method | Path | Body / result |
| --- | --- | --- |
| `GET` | `/api/v1/pads` | Connected virtual pads. |
| `POST` | `/api/v1/pads` | `{ "kind": "xbox360", "name": "…" }` — create a pad. |
| `DELETE` | `/api/v1/pads/{id}` | Remove one pad. |
| `GET` | `/api/v1/viiper` | VIIPER health, address, install hint. |
| `POST` | `/api/v1/viiper/action` | `{ "action": "start" \| "stop" \| "restart" \| "install" }`. |
| `POST` | `/api/v1/viiper/password` | Set the VIIPER API password used by the daemon. |

Gamepad input itself is not HTTP: the Deck sends compact binary state packets to
`47831/udp`, one per frame while streaming is on.

## Monitor

| Method | Path | Body / result |
| --- | --- | --- |
| `GET` | `/api/v1/monitor` | `MonitorStatus`: running, backend, position, width, height, fps, quality, output, error. |
| `POST` | `/api/v1/monitor/start` | `MonitorStart`: mode (`virtual`/`mirror`), backend, position, output, size, fps, quality, input flag. |
| `POST` | `/api/v1/monitor/stop` | Stops the monitor and removes the virtual output. |
| `POST` | `/api/v1/monitor/input` | Pointer/keyboard events for forwarded input. |
| `GET` | `/api/v1/monitor/frame` | Latest JPEG frame. |

Position values: `auto`, `right`, `left`, `above`, `below`.

## Notifications

| Method | Path | Body / result |
| --- | --- | --- |
| `GET` | `/api/v1/notifications` | Newest notifications plus the unread cursor. |
| `DELETE` | `/api/v1/notifications/{id}` | Dismiss one notification. |
| `POST` | `/api/v1/notifications/clear` | Clear all. |
| `POST` | `/api/v1/notifications/test` | Send a test notification. |

## Home Assistant

| Method | Path | Body / result |
| --- | --- | --- |
| `GET` | `/api/v1/ha/states` | Entities proxied from Home Assistant. |
| `POST` | `/api/v1/ha/config` | `{ "url": "…", "token": "…" }`. |
| `POST` | `/api/v1/ha/test` | Check the connection. |
| `POST` | `/api/v1/ha/call` | `{ "domain": "light", "service": "toggle", "entity_id": "light.kitchen" }`. |
| `POST` | `/api/v1/ha/favorites` | Pin or unpin entities. |

## Remote actions

`POST /api/v1/remote`

```json
{ "command": "volume_up" }
```

Allowed commands:

- `media_play_pause`, `media_next`, `media_prev`, `media_stop`
- `volume_up`, `volume_down`, `volume_mute`
- `lock`
- `monitor_start`, `monitor_stop`
- `open:https://…` (http and https only)
- `launch:<desktop-id>` (validated characters only)

Anything else returns `400`. There is no arbitrary shell execution endpoint.

## Security

| Method | Path | Body / result |
| --- | --- | --- |
| `POST` | `/api/v1/security/password` | Change the pairing password. |
| `POST` | `/api/v1/security/rotate` | Rotate the API token; existing Deck pairings must pair again. |

## Install

| Method | Path | Body / result |
| --- | --- | --- |
| `POST` | `/api/v1/install` | `{ "component": "viiper" \| "usbip" \| … }`. |
| `GET` | `/api/v1/install/status` | Progress of the current install job. |
