//! One capture per physical camera for the whole app (docs/DIRECTOR_COMPOSITOR.md, phase 2):
//! every output that shows a camera draws from the same frames, as OBS does, instead of each
//! window opening the device again.
//!
//! The bundled ffmpeg opens the device (avfoundation on macOS, dshow on Windows; the names and
//! matching are the live stream's, stream.rs) and hands over NV12, the format cameras and the
//! GPU compositor both use. The latest frame wins; nothing queues. A camera is closed a moment
//! after its last holder lets go, so switching looks doesn't reopen it.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use kairo_server::proc::Command;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long a camera stays open after its last holder lets go (video_hub.js keeps 1.5 s).
const LINGER: Duration = Duration::from_millis(1500);

static NEXT_INSTANCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The newest frame of a camera: NV12, tightly packed, `seq` counting frames.
pub struct Latest {
    pub width: u32,
    pub height: u32,
    pub nv12: Vec<u8>,
    pub seq: u64,
}

struct Running {
    /// Which opening of the camera this is: a hold only counts against the one it took.
    instance: u64,
    latest: Arc<Mutex<Latest>>,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    holders: usize,
    released: Option<Instant>,
}

impl Running {
    fn close(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut c) = self.child.lock() {
            if let Some(mut child) = c.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[derive(Default)]
pub struct Hub {
    running: Mutex<HashMap<String, Running>>,
}

/// A hold on a camera's frames; the camera may close once every hold is dropped.
pub struct Held {
    pub label: String,
    pub latest: Arc<Mutex<Latest>>,
    instance: u64,
    stop: Arc<AtomicBool>,
    hub: Arc<Hub>,
}

impl Held {
    /// False once the camera went (unplugged, or ffmpeg ended): its frames have stopped.
    pub fn alive(&self) -> bool {
        !self.stop.load(Ordering::Relaxed)
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Ok(mut map) = self.hub.running.lock() {
            if let Some(r) = map.get_mut(&self.label).filter(|r| r.instance == self.instance) {
                r.holders = r.holders.saturating_sub(1);
                if r.holders == 0 {
                    r.released = Some(Instant::now());
                }
            }
        }
        let hub = self.hub.clone();
        thread::spawn(move || {
            thread::sleep(LINGER + Duration::from_millis(50));
            hub.close_idle();
        });
    }
}

impl Hub {
    /// The camera named `label` (as the page names it), opened if it isn't already.
    pub fn hold(self: &Arc<Self>, ffmpeg: &Path, label: &str) -> Result<Held, String> {
        if let Some(h) = self.join(label)? {
            return Ok(h);
        }
        // Opened without the lock held (listing devices and waiting for the first frame can take
        // seconds, and every output's camera release waits on this lock).
        let opened = open(ffmpeg, label)?;
        let mut map = self.running.lock().map_err(|e| e.to_string())?;
        // Another output opened it meanwhile: use that one.
        if let Some(r) = map.get_mut(label).filter(|r| !r.stop.load(Ordering::Relaxed)) {
            opened.close();
            r.holders += 1;
            r.released = None;
            return Ok(self.held(label, r));
        }
        // One that went (unplugged) is ended properly before it is replaced.
        if let Some(gone) = map.remove(label) {
            gone.close();
        }
        let mut running = opened;
        running.instance = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
        let h = self.held(label, &running);
        map.insert(label.into(), running);
        Ok(h)
    }

    /// A hold on the camera if it is open and still sending.
    fn join(self: &Arc<Self>, label: &str) -> Result<Option<Held>, String> {
        let mut map = self.running.lock().map_err(|e| e.to_string())?;
        Ok(map.get_mut(label).filter(|r| !r.stop.load(Ordering::Relaxed)).map(|r| {
            r.holders += 1;
            r.released = None;
            self.held(label, r)
        }))
    }

    fn held(self: &Arc<Self>, label: &str, r: &Running) -> Held {
        Held { label: label.into(), latest: r.latest.clone(), instance: r.instance, stop: r.stop.clone(), hub: self.clone() }
    }

    fn close_idle(&self) {
        // Taken out under the lock, ended after it (ending waits for ffmpeg to exit).
        let done: Vec<Running> = match self.running.lock() {
            Ok(mut map) => {
                let keys: Vec<String> = map.iter()
                    .filter(|(_, r)| (r.holders == 0 && r.released.is_some_and(|t| t.elapsed() >= LINGER)) || r.stop.load(Ordering::Relaxed))
                    .map(|(k, _)| k.clone())
                    .collect();
                keys.iter().filter_map(|k| map.remove(k)).collect()
            }
            Err(_) => Vec::new(),
        };
        for r in done {
            r.close();
        }
    }
}

/// The size ffmpeg reports for the camera's video stream ("…, nv12, 1280x720, …").
fn stream_size(line: &str) -> Option<(u32, u32)> {
    if !line.contains("Video:") {
        return None;
    }
    line.split(|c: char| c == ',' || c == ' ').find_map(|t| {
        let (w, h) = t.split_once('x')?;
        let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
        (w >= 16 && h >= 16).then_some((w, h))
    })
}

fn open(ffmpeg: &Path, label: &str) -> Result<Running, String> {
    let fmt = crate::stream::device_format().ok_or("cameras can't be opened natively on this computer yet")?;
    let (videos, _) = crate::stream::list_devices(ffmpeg);
    let i = crate::stream::match_device_at(label, &videos, &HashSet::new(), None).ok_or_else(|| format!("\"{label}\" wasn't found"))?;
    let input = if fmt == "avfoundation" { i.to_string() } else { format!("video={}", videos[i]) };
    let mut child = Command::new(ffmpeg)
        // The camera's own size and rate; NV12 out, converted only if the camera gives another
        // format (most give NV12 or UYVY).
        .args(["-hide_banner", "-nostdin", "-loglevel", "info", "-f", fmt, "-i", &input, "-an", "-pix_fmt", "nv12", "-f", "rawvideo", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't start ffmpeg: {e}"))?;
    let mut out = child.stdout.take().ok_or("ffmpeg gave no output")?;
    let err = child.stderr.take().ok_or("ffmpeg gave no messages")?;

    // The size comes on stderr before the first frame; the rest of stderr is drained so ffmpeg
    // never blocks on it.
    let (size_tx, size_rx) = std::sync::mpsc::sync_channel::<(u32, u32)>(1);
    thread::spawn(move || {
        let mut sent = false;
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            if !sent {
                if let Some(s) = stream_size(&line) {
                    let _ = size_tx.send(s);
                    sent = true;
                }
            }
        }
    });
    let Ok((width, height)) = size_rx.recv_timeout(Duration::from_secs(8)) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("\"{label}\" didn't start"));
    };
    if width % 2 != 0 || height % 2 != 0 {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("\"{label}\" is {width}x{height}; NV12 needs even sides"));
    }

    let len = (width * height * 3 / 2) as usize;
    let latest = Arc::new(Mutex::new(Latest { width, height, nv12: vec![0; len], seq: 0 }));
    let stop = Arc::new(AtomicBool::new(false));
    {
        let (latest, stop) = (latest.clone(), stop.clone());
        thread::spawn(move || {
            let mut buf = vec![0u8; len];
            while !stop.load(Ordering::Relaxed) {
                if out.read_exact(&mut buf).is_err() {
                    break;
                }
                // Handed over by swapping, not copying.
                if let Ok(mut l) = latest.lock() {
                    std::mem::swap(&mut l.nv12, &mut buf);
                    l.seq += 1;
                }
            }
            // The camera went (unplugged, or ffmpeg ended): holders see no new frames, and the
            // next hold opens it again.
            stop.store(true, Ordering::Relaxed);
        });
    }
    eprintln!("[Camera] \"{label}\" open at {width}x{height}");
    Ok(Running { instance: 0, latest, stop, child: Arc::new(Mutex::new(Some(child))), holders: 1, released: None })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_stream_size_comes_from_the_video_line() {
        let l = "  Stream #0:0: Video: rawvideo (UYVY / 0x59565955), uyvy422, 1280x720, 1105920 kb/s, 30 fps, 30 tbr, 1000k tbn";
        assert_eq!(super::stream_size(l), Some((1280, 720)));
        assert_eq!(super::stream_size("  Stream #0:1: Audio: pcm_f32le, 48000 Hz, mono"), None);
        assert_eq!(super::stream_size("Input #0, avfoundation, from '0':"), None);
    }
}
