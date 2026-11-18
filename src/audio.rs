use crate::{track::Song, Result};
use rand::seq::SliceRandom;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::{fs::File, io::BufReader, path::{Path, PathBuf}, sync::{mpsc, Arc, Mutex}, thread, time::Duration};

// Décodage en flux : la pause suspend la consommation, le skip ferme FFmpeg.
struct OpusSource {
    child: std::process::Child,
    pcm: BufReader<std::process::ChildStdout>,
}

impl OpusSource {
    fn open(path: &Path) -> Result<Self> {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        let mut child = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-i"]).arg(path)
            .args(["-map", "0:a:0", "-vn", "-f", "s16le", "-ac", "2", "-ar", "48000", "pipe:1"])
            .stdin(Stdio::null()).stderr(Stdio::null()).stdout(Stdio::piped()).spawn()?;
        let stdout = child.stdout.take().expect("stdout configuré en pipe");
        let mut source = Self { child, pcm: BufReader::new(stdout) };
        if source.pcm.fill_buf()?.is_empty() { return Err("Aucun échantillon audio.".into()); }
        Ok(source)
    }
}

impl Iterator for OpusSource {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        use std::io::Read;
        let mut bytes = [0; 2];
        self.pcm.read_exact(&mut bytes).ok()?;
        Some(i16::from_le_bytes(bytes))
    }
}

impl rodio::Source for OpusSource {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { 2 }
    fn sample_rate(&self) -> u32 { 48000 }
    fn total_duration(&self) -> Option<Duration> { None }
}

impl Drop for OpusSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Player {
    // Le flux audio doit rester vivant pendant toute la lecture.
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Option<Sink>,
    queue: Vec<usize>,
    position: usize,
    shuffled: bool,
}

impl Player {
    fn new() -> Result<Self> {
        let (stream, handle) = OutputStream::try_default()?;

        Ok(Self {
            _stream: stream,
            handle,
            sink: None,
            queue: Vec::new(),
            position: 0,
            shuffled: false,
        })
    }

    fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
    }

    fn play_file(&self, path: &Path) -> Result<Sink> {
        let sink = Sink::try_new(&self.handle)?;
        if path.extension().and_then(|s| s.to_str()) == Some("opus") {
            sink.append(OpusSource::open(path)?);
        } else {
            sink.append(Decoder::new(BufReader::new(File::open(path)?))?);
        }
        Ok(sink)
    }

    fn play_current(&mut self, songs: &[Song]) -> String {
        self.stop();
        while let Some(&index) = self.queue.get(self.position) {
            let song = &songs[index];
            if let Ok(sink) = self.play_file(&song.path) {
                self.sink = Some(sink);
                return format!("♪ {}", song.label);
            }
            self.position += 1;
        }
        "Fin de lecture.".into()
    }
}

pub(crate) enum AudioCommand {
    Start(Arc<Vec<Song>>, usize, bool),
    Renamed(String, Song),
    Removed(Vec<String>),
    Toggle,
    Previous,
    Next,
}

#[derive(Clone, Default)]
pub(crate) struct Playback {
    pub(crate) status: String,
    pub(crate) current: Option<PathBuf>,
    pub(crate) paused: bool,
    pub(crate) shuffled: bool,
}

pub(crate) struct Audio {
    sender: Option<mpsc::Sender<AudioCommand>>,
    view: Arc<Mutex<Playback>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Audio {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let view = Arc::new(Mutex::new(Playback::default()));
        let shared = Arc::clone(&view);
        let worker = thread::spawn(move || audio_loop(receiver, shared));
        Self { sender: Some(sender), view, worker: Some(worker) }
    }

    pub(crate) fn send(&self, command: AudioCommand) -> Result<()> {
        self.sender.as_ref().ok_or("Lecteur fermé.")?
            .send(command).map_err(|_| "Lecteur audio indisponible.")?;
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> Playback {
        self.view.lock().unwrap().clone()
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

// Le thread possède à la fois la sortie audio et les titres de la file active.
fn audio_loop(receiver: mpsc::Receiver<AudioCommand>, view: Arc<Mutex<Playback>>) {
    let mut player: Option<Player> = None;
    let mut songs = Arc::new(Vec::<Song>::new());
    let mut status = String::new();

    loop {
        let command = match receiver.recv_timeout(Duration::from_millis(80)) {
            Ok(command) => Some(command),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if player.is_none() {
            if !matches!(&command, Some(AudioCommand::Start(..))) { continue; }
            match Player::new() {
                Ok(p) => player = Some(p),
                Err(e) => {
                    view.lock().unwrap().status = format!("Sortie audio : {e}");
                    continue;
                }
            }
        }
        let p = player.as_mut().unwrap();
        match command {
            Some(AudioCommand::Start(items, selected, shuffle)) => {
                songs = items;
                p.shuffled = shuffle;
                p.queue = if shuffle { (0..songs.len()).collect() }
                          else { (selected..songs.len()).collect() };
                if shuffle { p.queue.shuffle(&mut rand::thread_rng()); }
                p.position = 0;
                status = p.play_current(&songs);
            }
            Some(AudioCommand::Renamed(key, replacement)) => {
                for song in Arc::make_mut(&mut songs).iter_mut() {
                    if song.track.key() == key {
                        *song = replacement.clone();
                    }
                }
                if let Some(&index) = p.queue.get(p.position).filter(|_| p.sink.is_some()) {
                    status = format!("♪ {}", songs[index].label);
                }
            }
            Some(AudioCommand::Removed(keys)) => {
                let current_removed = p.queue.get(p.position)
                    .is_some_and(|&i| keys.contains(&songs[i].track.key()));
                let paused = p.sink.as_ref().is_some_and(|s| s.is_paused());
                let before = p.queue.iter().take(p.position)
                    .filter(|&&i| !keys.contains(&songs[i].track.key())).count();
                p.queue.retain(|&i| !keys.contains(&songs[i].track.key()));
                p.position = before;
                if current_removed {
                    status = p.play_current(&songs);
                    if paused { if let Some(sink) = &p.sink { sink.pause(); } }
                }
            }
            Some(AudioCommand::Toggle) => {
                if let Some(sink) = &p.sink {
                    if sink.is_paused() { sink.play(); } else { sink.pause(); }
                }
            }
            Some(AudioCommand::Previous) if !p.queue.is_empty() => {
                p.position = p.position.saturating_sub(1);
                status = p.play_current(&songs);
            }
            Some(AudioCommand::Next) if p.sink.is_some() => {
                p.position += 1;
                status = p.play_current(&songs);
            }
            _ => {}
        }

        if p.sink.as_ref().is_some_and(|sink| sink.empty()) {
            p.position += 1;
            status = p.play_current(&songs);
        }

        let current = p.sink.as_ref().and_then(|_| p.queue.get(p.position))
            .map(|&index| songs[index].path.clone());
        *view.lock().unwrap() = Playback {
            current,
            status: status.clone(),
            paused: p.sink.as_ref().is_some_and(|s| s.is_paused()),
            shuffled: p.shuffled && p.sink.is_some(),
        };
    }
}
