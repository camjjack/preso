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

Before an inline clip plays, the slide shows the clip's **first frame** —
unless the slide has a picture of its own (an `![](…)` image, an `image:` layer
or a full-bleed [`background=`](images.md#full-bleed-backgrounds)), which then
stands as the poster until you start the clip. Display math doesn't count as a
poster. Inline playback draws the video over the slide.

A slide played by an external player (or without the `video` feature) can't
show frames, so it shows your poster with a centered ▶ play badge instead —
add a poster there to fill the frame. The exported PDF always shows the poster
and badge.

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
clips stay paused until you start them; until then the slide shows the first
frame (or the poster). Pausing holds the current frame on the slide, so you
can stop on a frame and talk about it. Editing the deck reloads only clips it
newly references, so the authoring loop doesn't re-pay the cost.

Controls (on the slide's clip):

| Key | Action |
|-----|--------|
| <kbd>Space</kbd> or <kbd>v</kbd> | Play / pause |
| <kbd>←</kbd> | While playing, scrub back a few seconds |
| <kbd>⌥</kbd><kbd>←</kbd> | While playing, rewind to the start |
| <kbd>,</kbd> / <kbd>.</kbd> | Pause and step one frame back / on |

The presenter window shows a **scrub bar** under the current slide while it has
an inline clip: a small play / pause button, elapsed time, a bar to drag, and
the clip's length. Drag it to
move through the clip — the audience sees each frame you pass, paused or
playing — and let go where you want it; pressing play carries on from there,
even after the clip has reached its end.

Leaving the slide stops the clip (it stays loaded); come back and it shows the
frame it stopped on. The presenter's current-slide preview shows the same frame
as the audience window and plays along with it, so you can see what they see
and pause where you mean to; its status line shows the play / pause and rewind
hints. The audience sees just the video, with no play button over it — the
controls live in the presenter window. Because <kbd>Space</kbd> controls the clip on a
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
