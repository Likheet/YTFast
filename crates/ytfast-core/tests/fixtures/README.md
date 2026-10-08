# Test fixtures

Replies from YouTube Music's internal API, used to test the readers in
`src/read/`.

| File | Source |
|---|---|
| `playlist_signed_in_premium.json` | Real reply (March 2024), signed in with Premium. Trimmed from ytmusicapi's `tests/data/2024_03_get_playlist.json`. |
| `playlist_collaborative.json` | Real reply (October 2025), signed out, collaborative playlist with the same song twice. Trimmed from ytmusicapi's `tests/data/2025_10_get_playlist_collaborative.json`. |
| `playlist_signed_out.json` | Real reply (December 2025), signed out. Trimmed from ytmusicapi's `tests/data/2025_12_get_playlist_audio.json`. |
| `history_synthetic.json` | Written by hand in the History layout ytmusicapi reads (`singleColumnBrowseResultsRenderer` → `musicShelfRenderer`). Not a real reply. |
| `account_menu_synthetic.json` | Written by hand in the `account/account_menu` layout ytmusicapi reads. Not a real reply. |
| `player_synthetic.json` | Written by hand from the `player` reply example in ytmusicapi's documentation. Not a real reply. |
| `explore.json` | Real Explore page (November 2025). Trimmed from ytmusicapi's `tests/data/2025_11_get_explore.json`. |
| `album.json` | Real album page (May 2026). Trimmed from ytmusicapi's `tests/data/2026_05_get_album.json`. |
| `artist.json` | Real artist page (May 2026). Trimmed from ytmusicapi's `tests/data/2026_05_get_artist1.json`. |
| `home_synthetic.json` | Written by hand in the Home layout (carousels of list rows and of cards). Not a real reply. |
| `search_synthetic.json` | Written by hand in the search layout (top result card, song and album shelves). Not a real reply. |
| `search.json` | Real search reply (October 2026), signed out, for "coldplay": the top result card (an artist, its three songs and its Shuffle and Mix buttons), then one result per `itemSectionRenderer`. Trimmed to the card and the first six results, with tracking data dropped and at most two sizes of each picture. |
| `search_suggestions.json` | Real `music/get_search_suggestions` reply (October 2026), signed out, for "coldp": six suggestions, then an artist and a song with pictures. Trimmed to those two rows, with tracking data and the song's menu dropped. |
| `next_synthetic.json` | Written by hand in the Up next layout ytmusicapi reads (`playlistPanelRenderer`, with a song/video pair and an unplayable row), with a queue header ("Playing from") in the shape the live page shows. Not a real reply. |

Trimming kept the structure the readers walk and dropped tracking blobs,
long descriptions and all but the first few rows or cards of each list.

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
