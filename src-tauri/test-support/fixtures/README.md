# Synthetic audio fixture

`tone.flac` is a generated 440 Hz sine wave, not a recording. It contains no
third-party audio. Tests copy it into isolated temporary directories before
changing tags or scanning. They never play it through an output device.

Regenerate with FFmpeg (FFmpeg is not needed to run the tests):

```sh
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100 -t 0.25 -c:a flac -fflags +bitexact -flags:a +bitexact -metadata title="Fixture song" -metadata artist="Alice; Bob" -metadata album="Fixture album" -metadata album_artist="Ensemble" -metadata date="2024" -metadata track="3" -metadata disc="1" -metadata genre="Test" -metadata lyrics="[00:01.00]Fixture lyric" tone.flac
```
