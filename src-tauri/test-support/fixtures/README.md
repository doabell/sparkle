# Synthetic audio fixtures

`tone.flac` is a generated 440 Hz sine wave, not a recording. It contains no
third-party audio. Tests copy it into isolated temporary directories before
changing tags or scanning. They never play it through an output device.

`tone.mp3`, `tone.m4a`, `tone.ogg`, `tone.aac`, `tone-alac.m4a`, and `tone.opus`
are transcoded copies used to verify that
dependency upgrades preserve stored audio fingerprints across supported formats.
The expected fingerprints were captured before upgrading Symphonia 0.5.5.

Regenerate with FFmpeg (FFmpeg is not needed to run the tests):

```sh
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100 -t 0.25 -c:a flac -fflags +bitexact -flags:a +bitexact -metadata title="Fixture song" -metadata artist="Alice; Bob" -metadata album="Fixture album" -metadata album_artist="Ensemble" -metadata date="2024" -metadata track="3" -metadata disc="1" -metadata genre="Test" -metadata lyrics="[00:01.00]Fixture lyric" tone.flac
ffmpeg -i tone.flac -map_metadata -1 -c:a libmp3lame -fflags +bitexact -flags:a +bitexact tone.mp3
ffmpeg -i tone.flac -map_metadata -1 -c:a aac -fflags +bitexact -flags:a +bitexact tone.m4a
ffmpeg -i tone.flac -map_metadata -1 -c:a libvorbis -fflags +bitexact -flags:a +bitexact tone.ogg
ffmpeg -i tone.flac -map_metadata -1 -c:a aac -fflags +bitexact -flags:a +bitexact tone.aac
ffmpeg -i tone.flac -map_metadata -1 -c:a alac -fflags +bitexact -flags:a +bitexact tone-alac.m4a
ffmpeg -i tone.flac -map_metadata -1 -c:a libopus -fflags +bitexact -flags:a +bitexact tone.opus
```

Encoder versions can change the bytes. Keep the checked-in fixtures when checking
compatibility with existing fingerprints; regenerate only for an intentional
fixture change and verify the new baseline using the old parser first.
