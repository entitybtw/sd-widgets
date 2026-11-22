# PC audio on the Deck

The daemon can pour the computer's sound — everything, or only the applications
you pick — into the Steam Deck speakers over the LAN. It is off by default: the
user turns it on in the daemon, and the Deck asks for confirmation before it
starts.

## How it works

```
app / system sink ──▶ parec (s16le PCM) ──▶ HTTP stream ──▶ pw-play on the Deck
```

- The daemon records with `parec` (PulseAudio protocol, which PipeWire
  implements) and serves raw `s16le` PCM at 44.1 or 48 kHz, stereo.
- `system` records `@DEFAULT_MONITOR@`, so it captures what the computer is
  currently playing on its default output.
- `apps` starts one `pw-record --target=<sink-input>` per stream of the chosen
  applications and sums the samples in Rust (20 ms frames). Nothing on the
  computer is re-routed, so the applications keep playing to their normal
  output and the same sound also lands on the Deck. On plain PulseAudio, where
  `pw-record` does not exist, it falls back to a temporary null sink `sdw_audio`
  and moves the streams back when the stream stops.
- The Deck reads `GET /api/v1/audio/stream` with its normal bearer token and
  pipes the bytes into `pw-play` (falling back to `paplay` or `aplay`). Volume
  is applied by the player, so no codec is linked into the app.

There is no compression: 48 kHz stereo s16 is about 1.5 Mbit/s. That is fine on a
local network and keeps latency low (the capture is requested with a 30 ms
buffer).

## Requirements

| Side | Need |
| --- | --- |
| Computer | PipeWire or PulseAudio, `pactl` and `parec` (`pipewire-pulse` or `pulseaudio-utils`); `pw-record` for per-application capture |
| Deck | `pw-play`, or `paplay`, or `aplay` |

If `pactl` is missing, the daemon reports `available: false` and the switch in
the daemon window explains what to install.

## Turning it on

1. Daemon window → **Audio** → **allow audio**. Nothing is captured while this
   is off. **Start from the deck** stays on unless you want the daemon window to
   be the only place that can start a stream.
2. Deck → **Audio** page → choose **whole computer** or **applications**,
   refresh the list, tick one or more apps, choose 44.1/48 kHz.
3. Press **start audio**. The Deck asks "Are you sure you want to move the
   computer's sound to the deck?" as many times as **confirmations** is set
   (0–3, in **Settings → pc audio** and on the Audio page).
4. Press **stop** on either side. Application streams return to the computer's
   normal output.

While streaming, those applications are quiet on the computer: the sound has
moved to the Deck. The page shows the source, sample rate, channels, transferred
megabytes and seconds.

## API

| Method | Path | Meaning |
| --- | --- | --- |
| `GET` | `/api/v1/audio` | `AudioStatus`: available, backend, running, source, apps, rate, channels, bytes, seconds, error. |
| `GET` | `/api/v1/audio/sources` | Applications currently producing sound: name, binary, sink-input index, media name. |
| `POST` | `/api/v1/audio/start` | `AudioStart`: `{ source, apps, rate, channels }`. `400` when disabled, remote start is off, or no chosen app is playing. |
| `POST` | `/api/v1/audio/stop` | Stop capture and put application streams back. |
| `GET` | `/api/v1/audio/stream` | Raw PCM, `audio/L16;rate=…;channels=…`. `409` when idle. |

`GET /api/v1/status` also carries `audio`, so the Deck can show the state without
a second request. Every route needs the paired bearer token; there is no
unauthenticated audio endpoint.

## Settings

Daemon (`daemon.toml`):

```toml
[audio]
enabled = false
allow_remote = true
rate = 48000
channels = 2
```

Deck (`deck.json`):

```json
{
  "audio": {
    "volume": 80,
    "confirmations": 1,
    "source": "system",
    "apps": [],
    "rate": 48000
  }
}
```

`confirmations` is a Deck-side consent step, not a daemon permission: it makes
accidental taps harmless. `allow_remote = false` on the daemon is the real lock;
the Deck then gets a clear error instead of a stream.

## Visualizer

The Deck draws a cava-style meter in the top bar, the control centre, the clock
and the full-screen clock. Its bands come from the shared analyzer in
`sdw-core::spectrum` (a 2048-point FFT, 24 logarithmic bands, 0..=1 over
−60…0 dB). When the Deck plays the computer's PCM it analyzes that. When the
sound stays on the computer, `GET /api/v1/spectrum` returns the same bands from
the daemon's own capture: `parec` on the default sink's monitor, started by the
first request and stopped six seconds after the last, so an idle machine runs
nothing. If the daemon has no such endpoint (an older build), the Deck falls
back to its local meter and to the track's motion.

`[audio] visualizer = false` in the daemon config turns the monitor capture off.
The Deck side is `VizCfg`: placement (`nav`, `shade`, `clock`), source (`pc`,
`media`), shape (`bars`, `dots`, `wave`), `bars`, `gap`, `width`, `height`,
`opacity`, `gain` (sensitivity), `mirror`, `peak` and `color`. The defaults
follow cava, and the **cava look** button restores them.

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| `available: false` | `pactl` or `parec` missing; install `pipewire-pulse` or `pulseaudio-utils`. |
| `выбранные приложения сейчас не играют звук` | The app has no active sink-input; start playback and refresh. |
| Silence, but the stream runs | The Deck player is missing or muted; check `pw-play`, the Deck volume and the Audio page volume. |
| Apps did not return to the speakers | Only the PulseAudio fallback re-routes streams; on PipeWire nothing was moved. In the fallback, stop from the daemon window so it moves every tracked stream back and unloads `sdw_audio`. |
| Player exits immediately | A stale daemon stream or an audio device change; press start again. |
