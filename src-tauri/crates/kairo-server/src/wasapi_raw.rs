//! The sound input on Windows, as the device hears it: opened with WASAPI's raw stream option, so
//! none of Windows' own processing (Voice Clarity, Voice Focus, noise suppression, automatic gain,
//! the device's "audio enhancements") reaches Kairo.
//!
//! Church PC, 22 Nov 2026: with Voice Clarity on the USB input, the choir was removed as noise and
//! Kairo was handed near-silence, much of it exact digital zeros, for seven minutes; the speech that
//! did come through sat at full scale whoever spoke (the automatic gain). The owner restarted
//! Kairo several times; the sound came back only when someone spoke again. cpal (native_audio.rs)
//! opens the input without the raw option, so it gets whatever Windows' effects leave.
//!
//! A device that refuses the raw option is opened without it (the log says so), as cpal would.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::native_audio::mono;
use windows::Win32::Devices::Properties;
use windows::Win32::Foundation::FALSE;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, AudioCategory_Other, AudioClientProperties, IAudioCaptureClient, IAudioClient2, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMOPTIONS_RAW,
    DEVICE_STATE_ACTIVE, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::Media::KernelStreaming::WAVE_FORMAT_EXTENSIBLE;
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, StructuredStorage, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::System::Variant::VT_LPWSTR;

/// A raw capture running on its own thread; stopped when dropped.
pub struct RawCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for RawCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// What was opened: the device's name, its rate and channels, and whether Windows' processing is
/// off (`raw`) or the device refused that.
pub struct RawOpened {
    pub name: String,
    pub rate: u32,
    pub channels: usize,
    pub raw: bool,
    pub capture: RawCapture,
}

/// Opens the input `matches` accepts (by its name; the default input when `want_default`), and
/// sends its blocks, mono f32, to `tx`. A failure while capturing sets `failed` and `why`.
pub fn open(
    matches: impl Fn(&str) -> bool + Send + 'static,
    want_default: bool,
    tx: mpsc::Sender<Vec<f32>>,
    failed: Arc<AtomicBool>,
    why: Arc<Mutex<String>>,
) -> Result<RawOpened, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(String, u32, usize, bool), String>>();
    let thread = std::thread::Builder::new()
        .name("kairo-capture-raw".into())
        .spawn(move || unsafe {
            if let Err(e) = run(&matches, want_default, &tx, &stop_t, &ready_tx) {
                // Before it was ready: the caller hears it. After: the capture loop sees `failed`.
                let _ = ready_tx.send(Err(e.clone()));
                *crate::util::lock(&why) = e;
                failed.store(true, Ordering::Relaxed);
            }
        })
        .map_err(|e| e.to_string())?;
    let capture = RawCapture { stop, thread: Some(thread) };
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok((name, rate, channels, raw))) => Ok(RawOpened { name, rate, channels, raw, capture }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("the input didn't start".into()),
    }
}

unsafe fn run(
    matches: &dyn Fn(&str) -> bool,
    want_default: bool,
    tx: &mpsc::Sender<Vec<f32>>,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<(String, u32, usize, bool), String>>,
) -> Result<(), String> {
    CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(e("COM"))?;
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e("devices"))?;
    let device: IMMDevice = if want_default {
        enumerator.GetDefaultAudioEndpoint(eCapture, eConsole).map_err(e("default input"))?
    } else {
        let list = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE).map_err(e("inputs"))?;
        let count = list.GetCount().map_err(e("inputs"))?;
        (0..count)
            .filter_map(|i| list.Item(i).ok())
            .find(|d| name_of(d).is_some_and(|n| matches(&n)))
            .ok_or_else(|| "no such input".to_string())?
    };
    let name = name_of(&device).unwrap_or_else(|| "Input".into());
    let client: IAudioClient2 = device.Activate(CLSCTX_ALL, None).map_err(e("open"))?;
    // Windows' processing off. Asked before Initialize, as the API requires.
    let props = AudioClientProperties {
        cbSize: std::mem::size_of::<AudioClientProperties>() as u32,
        bIsOffload: FALSE,
        eCategory: AudioCategory_Other,
        Options: AUDCLNT_STREAMOPTIONS_RAW,
    };
    let raw = client.SetClientProperties(&props).is_ok();
    let format = client.GetMixFormat().map_err(e("format"))?;
    let result = capture(&client, format, &name, raw, tx, stop, ready);
    CoTaskMemFree(Some(format as *const _));
    result
}

unsafe fn capture(
    client: &IAudioClient2,
    format: *mut WAVEFORMATEX,
    name: &str,
    raw: bool,
    tx: &mpsc::Sender<Vec<f32>>,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<(String, u32, usize, bool), String>>,
) -> Result<(), String> {
    let f = *format;
    let channels = f.nChannels.max(1) as usize;
    let bits = f.wBitsPerSample;
    // The struct is packed: its fields are read without assuming they're aligned.
    let tag = u32::from(std::ptr::addr_of!(f.wFormatTag).read_unaligned());
    let sub = (tag == WAVE_FORMAT_EXTENSIBLE).then(|| std::ptr::addr_of!((*(format as *const WAVEFORMATEXTENSIBLE)).SubFormat).read_unaligned());
    let float = tag == WAVE_FORMAT_IEEE_FLOAT || sub == Some(KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);
    if !(float && bits == 32) && !(!float && (bits == 16 || bits == 32)) {
        return Err(format!("{name}: unsupported sample format ({bits}-bit)"));
    }
    // 100 ms of buffer, in 100-ns units; polled every 10 ms.
    client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1_000_000, 0, format, None).map_err(e(name))?;
    let cap: IAudioCaptureClient = client.GetService().map_err(e(name))?;
    client.Start().map_err(e(name))?;
    let _ = ready.send(Ok((name.to_string(), f.nSamplesPerSec, channels, raw)));
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
        loop {
            let waiting = match cap.GetNextPacketSize() {
                Ok(n) => n,
                Err(err) => return Err(lost(err, name)),
            };
            if waiting == 0 {
                break;
            }
            let (mut data, mut frames, mut flags) = (std::ptr::null_mut::<u8>(), 0u32, 0u32);
            if let Err(err) = cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None) {
                return Err(lost(err, name));
            }
            let n = frames as usize;
            let block: Vec<f32> = if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                vec![0.0; n]
            } else if float {
                mono(std::slice::from_raw_parts(data as *const f32, n * channels), channels, |s| s)
            } else if bits == 16 {
                mono(std::slice::from_raw_parts(data as *const i16, n * channels), channels, |s| s as f32 / 32768.0)
            } else {
                mono(std::slice::from_raw_parts(data as *const i32, n * channels), channels, |s| s as f32 / 2_147_483_648.0)
            };
            let _ = cap.ReleaseBuffer(frames);
            let _ = tx.send(block);
        }
    }
    let _ = client.Stop();
    Ok(())
}

/// An error, said with what was being done.
fn e(what: &str) -> impl FnOnce(windows::core::Error) -> String + '_ {
    move |err| format!("{what}: {err}")
}

/// The system's words for a capture that stopped.
fn lost(err: windows::core::Error, name: &str) -> String {
    if err.code() == AUDCLNT_E_DEVICE_INVALIDATED {
        format!("{name}: the requested device is no longer available")
    } else {
        format!("{name}: {err}")
    }
}

/// A device's friendly name, as cpal reads it (so the same input matches by the same name).
unsafe fn name_of(device: &IMMDevice) -> Option<String> {
    let store = device.OpenPropertyStore(STGM_READ).ok()?;
    let mut value = store.GetValue(&Properties::DEVPKEY_Device_FriendlyName as *const _ as *const _).ok()?;
    let v = &value.as_raw().Anonymous.Anonymous;
    if v.vt != VT_LPWSTR.0 {
        return None;
    }
    let ptr = *(&v.Anonymous as *const _ as *const *const u16);
    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let name = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
    StructuredStorage::PropVariantClear(&mut value).ok();
    Some(name)
}
