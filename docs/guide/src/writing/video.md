# Video

A slide can play a video clip. Mark it with `<!-- video: … -->`:

```markdown
# Live Demo

![the result](demo-poster.png)

<!-- video: clips/demo.mp4 -->
```

The path is relative to the deck file (the same as images and includes).

There are two ways the clip plays, depending on how preso was built:

- **Inline** — the video plays *on the slide* (with audio). Needs a build with
  the `video` feature and the wgpu backend (the default). See
  [Inline playback](#inline-playback).
- **External** — preso hands the clip to a fullscreen external player. The
  fallback used when the `video` feature isn't compiled in, or you run with
  `--software`. See [External playback](#external-playback).

In both cases the slide shows a centered ▶ play badge (also in the presenter
preview and the exported PDF). While an inline clip is playing, the presenter's
badge and status line switch to ⏸ (`⏸ V: pause video`) so you can tell at a
glance that it's running on the audience window. There's no automatic
thumbnail, so add your own poster — an `![](…)` image or a full-bleed
[`background=`](images.md#full-bleed-backgrounds) — to fill the frame; inline
playback draws the video over it.

## Inline playback

Inline video uses [`iced_video_player`](https://crates.io/crates/iced_video_player)
(GStreamer decoding → a wgpu texture), so it requires:

1. **The wgpu backend** — the default. (Running `--software` forces the external
   player instead.)
2. **A build with the `video` feature**, which is *not* on by default because it
   pulls in GStreamer:

   ```sh
   cargo build --release --features video
   ```

3. **GStreamer installed** on the build *and* run machine. On macOS:

   ```sh
   brew install gstreamer gst-plugins-base gst-plugins-good \
                gst-plugins-bad gst-plugins-ugly pkg-config
   ```

   On Linux, install your distro's `gstreamer1.0` runtime + plugin packages.
   A binary built with `--features video` will not start on a machine without
   the GStreamer libraries, so this build is for people who have them (or a
   bundled distribution) — the plain binary stays dependency-free.

Every clip in the deck is **preloaded when the deck loads** (and prerolled, so
the first frame is ready), rather than on demand — so the first press plays
instantly instead of stalling mid-talk while GStreamer builds the pipeline. The
clips stay paused until you start them; until then the slide shows the poster +
▶ badge. Editing the deck reloads only clips it newly references, so the
authoring loop doesn't re-pay the cost.

Controls (on the slide's clip):

| Key | Action |
|-----|--------|
| <kbd>Space</kbd> or <kbd>v</kbd> | Play / pause |
| <kbd>←</kbd> | While playing, scrub back a few seconds |
| <kbd>⌥</kbd><kbd>←</kbd> | While playing, rewind to the start |

Leaving the slide stops the clip (it stays loaded). While a clip plays the
presenter's badge and status line show the ⏸ / rewind hints; the audience just
sees the video over the slide. Because <kbd>Space</kbd> controls the clip on a
video slide, advance with <kbd>→</kbd> / <kbd>PageDown</kbd> and step back with
<kbd>PageUp</kbd> / <kbd>Backspace</kbd> / <kbd>↑</kbd>.

> Preloading trades a little launch time and memory (all pipelines are held at
> once) for a freeze-free presentation. The first launch after installing
> GStreamer is slower still — GStreamer builds its plugin registry once (cached
> afterwards), and may print harmless warnings to the terminal (missing
> GObject-introspection typelibs, duplicate GTK classes from the bundled
> plugins). Neither affects playback.

## External playback

Without the `video` feature (or under `--software`), <kbd>v</kbd> launches an
external player for the current slide's clip:

1. [`mpv`](https://mpv.io) with `--fullscreen`, if it's on your `PATH` — the
   recommended setup: it opens borderless fullscreen, ideal for the audience
   monitor. Move the mpv window to that monitor once and it reopens there.
2. Otherwise, your OS default opener (`open` on macOS, `xdg-open` on Linux,
   `start` on Windows), which plays the file in whatever app is registered.

Close the player (or press <kbd>q</kbd> in mpv) to return to the deck. If the
file is missing, a problem banner appears on the presenter slide.

> The exported PDF can't hold video, so a page always shows the poster + ▶
> badge regardless of build.
