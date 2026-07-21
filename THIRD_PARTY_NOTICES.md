# Third-party notices

Sparkle includes or adapts portions of the projects below. Sparkle's own code
remains available under the MIT License in `LICENSE`; these notices apply to
the identified third-party portions.

## MusicBee-NeteaseLyrics

- Repository: <https://github.com/cqjjjzr/MusicBee-NeteaseLyrics>
- License: Apache License 2.0 (`Apache-2.0`)
- Sparkle file: `src-tauri/src/providers/lyrics/netease.rs`
- Status: adapted and substantially modified for Sparkle.

## MusicBee-QQLyrics

- Repository: <https://github.com/mslxl/MusicBee-QQLyrics>
- License: Apache License 2.0 (`Apache-2.0`)
- Sparkle file: `src-tauri/src/providers/lyrics/qq.rs`
- Status: adapted and substantially modified for Sparkle.
- This project is a fork/derivative of MusicBee-NeteaseLyrics, so the
  MusicBee-NeteaseLyrics and ZonyLrcToolsX notices also apply to this lineage.

## ZonyLrcToolsX

- Repository: <https://github.com/real-zony/ZonyLrcToolsX>
- License: MIT
- Sparkle file: `src-tauri/src/providers/lyrics/netease.rs`
- Copyright notice: Copyright (c) 2019 Zony.
- The NetEase encryption implementation in MusicBee-NeteaseLyrics identifies
  this project as an upstream source.

## mb_KashiNaviLyricsPlugin

- Repository: <https://github.com/noriokun4649/mb_KashiNaviLyricsPlugin>
- License: MIT
- Sparkle file: `src-tauri/src/providers/lyrics/kashinavi.rs`
- Copyright notice: Copyright (c) 2019 noriokun4649.
- The upstream README says this project was based on htsign's MusicBee plugin
  template; that provenance is listed below.

## MusicBeePluginTemplate

- Repository: <https://github.com/htsign/MusicBeePluginTemplate>
- License: MIT
- Relevance: upstream template credited by mb_KashiNaviLyricsPlugin.

## DiscordBee

- Repository: <https://github.com/sll552/DiscordBee>
- License: Apache License 2.0 (`Apache-2.0`)
- Sparkle file: `src-tauri/src/discord.rs`
- Status: Discord presence portions were adapted and substantially modified
  for Sparkle. Sparkle's Catbox artwork integration and related changes are
  original Sparkle work.

## License texts

The complete Apache License 2.0 text is included in
`licenses/Apache-2.0.txt`. The complete MIT text for the directly adapted
projects, including the relevant copyright notices, is included below.

### MIT License — ZonyLrcToolsX

Copyright (c) 2019 Zony

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

### MIT License — mb_KashiNaviLyricsPlugin

Copyright (c) 2019 noriokun4649

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

MusicBeePluginTemplate is listed as provenance for mb_KashiNaviLyricsPlugin;
no template file is copied directly into Sparkle.
