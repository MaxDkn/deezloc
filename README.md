# DeezLoc

A Rust terminal music player built with Ratatui. Import public Deezer or Spotify playlists, find matching tracks on YouTube, and listen offline.

## Quick start

On macOS, install the dependencies:

```bash
brew install rust yt-dlp ffmpeg deno
```

For Spotify imports, also install spotDL:

```bash
brew install pipx
pipx ensurepath
pipx install spotdl
```

Restart your terminal if `spotdl` is not found. Then build and launch DeezLoc from the project directory:

```bash
cargo build --release
./target/release/deezloc
```

Choose **Ajouter une playlist**, select **Deezer** or **Spotify**, and paste a public playlist URL. Spotify album links are also supported.

Select a saved playlist to open its available tracks directly. Press **`i`** on a playlist to rename it, download missing tracks, add a YouTube track, or delete it.

Tracks are saved as **Opus at 64 kbit/s**. Downloads run up to **10 at a time**, and existing audio files are verified before being skipped. The search uses the artist, title, and the word “audio”; incorrect matches can be replaced from the track’s Info popup.

## Controls

| Control | Action |
| --- | --- |
| ↑ / ↓ or mouse wheel | Browse the list. |
| Enter | Open a playlist, play a track, or activate the selected field. |
| `f` | Search tracks by title or artist. Enter plays a result; Esc closes the search. |
| `a` | Start shuffled playback, with each available track played once. |
| Space | Pause or resume; start the selected track if playback is idle. |
| ← / → | Previous or next track. `n` also skips forward. |
| `i` | Open information for the selected playlist or track. |
| `d` | Request deletion from an Info popup, with confirmation. |
| Esc / `q` | Go back. From the main menu, open the quit confirmation. |

Playback continues while browsing other playlists. Returning to the main menu preserves your selection and scroll position. Playback buttons also support mouse clicks.

## Information and editing

Info popups appear over the current list. Browse all fields with ↑ / ↓ and press Enter to edit supported fields.

- **Playlists:** edit the name, download missing tracks, or add a track using a YouTube URL.
- **Tracks:** edit the title or artist, or replace the audio using a different YouTube URL.

Text fields start with their current value; the YouTube URL input starts empty. Track corrections apply across local playlists and update the audio filenames and metadata.

Deleting a playlist or removing a track preserves audio files still referenced by another local playlist. Changes remain local and do not modify Deezer or Spotify.

## Command-line downloads

Pass a CSV path or public Deezer URL to download without opening the interface:

```bash
./target/release/deezloc playlist.csv 20
./target/release/deezloc "https://www.deezer.com/playlist/123456789"
```

| Argument | Meaning |
| --- | --- |
| CSV path or Deezer URL | Playlist to process. |
| Optional number | Process the first N CSV entries, including already downloaded tracks. Omit it for the entire playlist. |

Running without arguments opens the interactive interface. Spotify imports are available through the interface.

CSV files must contain **`Track name`** and **`Artist name`** columns. Place `playlist.csv` in the project directory to display it in the menu, or pass another path on the command line.

## Storage and configuration

- `.playlists/` — imported playlists and their names.
- `.musics/` — downloaded audio and saved track corrections.
- `src/config.rs` — audio bitrate (`OPUS_BITRATE`), download concurrency (`MAX_DOWNLOADS`), and audio directory (`MUSIC_DIR`).

Run DeezLoc from the project directory so it uses the same playlists and audio collection.