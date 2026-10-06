# Test fixtures

Replies from YouTube Music's internal API, used to test `src/read.rs`.

| File | Source |
|---|---|
| `playlist_signed_in_premium.json` | Real reply (March 2024), signed in with Premium. Trimmed from ytmusicapi's `tests/data/2024_03_get_playlist.json`. |
| `playlist_collaborative.json` | Real reply (October 2025), signed out, collaborative playlist with the same song twice. Trimmed from ytmusicapi's `tests/data/2025_10_get_playlist_collaborative.json`. |
| `playlist_signed_out.json` | Real reply (December 2025), signed out. Trimmed from ytmusicapi's `tests/data/2025_12_get_playlist_audio.json`. |
| `history_synthetic.json` | Written by hand in the History layout ytmusicapi reads (`singleColumnBrowseResultsRenderer` → `musicShelfRenderer`). Not a real reply. |
| `account_menu_synthetic.json` | Written by hand in the `account/account_menu` layout ytmusicapi reads. Not a real reply. |
| `player_synthetic.json` | Written by hand from the `player` reply example in ytmusicapi's documentation. Not a real reply. |

Trimming kept the structure the readers walk and dropped thumbnails,
tracking blobs and all but the first rows.

ytmusicapi (https://github.com/sigma67/ytmusicapi) is MIT licensed,
Copyright (c) 2026 sigma67.

When `ytfast-check` meets a reply these readers cannot handle, save a real
reply here (without anything personal in it) and add a test.

`tone_dash.m4a` is a 4-second 440 Hz tone made with ffmpeg in the layout of
YouTube's audio streams (fragmented MP4: `ftyp`, `moov`, `sidx`, then
`moof`/`mdat` pairs), to test decoding and seeking:

```sh
ffmpeg -f lavfi -i "sine=frequency=440:duration=4:sample_rate=44100" -ac 2 \
  -c:a aac -b:a 32k -movflags +dash+global_sidx -frag_duration 1000000 \
  -map_metadata -1 -fflags +bitexact -flags:a +bitexact -f mp4 tone_dash.m4a
```
