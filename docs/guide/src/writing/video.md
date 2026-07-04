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
preview and the exported PDF), and the presenter status line shows
`▶ V: play video`. There's no automatic thumbnail, so add your own poster — an
`![](…)` image or a full-bleed [`background=`](images.md#full-bleed-backgrounds)
— to fill the frame; inline playback draws the video over it.

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

The clip loads lazily on the **first** <kbd>v</kbd> press (so paging past video
slides never pays the GStreamer decode cost), then plays; press <kbd>v</kbd>
again to pause and resume. Until then the slide shows the poster + ▶ badge.
Leaving the slide stops and unloads the clip.

> The first ever inline load after installing GStreamer is slow — GStreamer
> builds its plugin registry once (cached afterwards), and may print harmless
> warnings to the terminal (missing GObject-introspection typelibs, duplicate
> GTK classes from the bundled plugins). Neither affects playback.

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
