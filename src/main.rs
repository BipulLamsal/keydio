use anyhow::Result;
use audrey::read::Reader;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, FromSample, Host, Sample, SampleFormat, SizedSample, StreamConfig,
    SupportedStreamConfig,
};
use device_query::{DeviceEvents, DeviceState, Keycode};
use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
};

const ASSETS: &str = "assets";
const AUDIOFILE: [(&str, SoundType); 5] = [
    ("BACKSPACE.mp3", SoundType::Backspace),
    ("ENTER.mp3", SoundType::Enter),
    ("GENERIC_R0.mp3", SoundType::Generic),
    ("GENERIC_R1.mp3", SoundType::Generic),
    ("SPACE.mp3", SoundType::Space),
];

const CHERRYMXBROWN: &str = "cherrymxbrown";

#[derive(Default)]
enum Theme {
    #[default]
    CherryMXBrown,
}

struct AudioHandler {
    host: Host,
    device: Device,
    config: Arc<SupportedStreamConfig>,
    rx: Receiver<(KeyPressType, SoundType)>,
}

impl AudioHandler {
    fn new(
        host: Host,
        device: Device,
        config: SupportedStreamConfig,
        rx: Receiver<(KeyPressType, SoundType)>,
    ) -> Self {
        Self {
            host,
            device,
            config: Arc::new(config),
            rx,
        }
    }

    fn stream_on_press(&self, app: Arc<Mutex<AppState>>) -> Result<()> {
        let config = &self.config;
        match config.sample_format() {
            SampleFormat::I8 => self.run::<i8>(&config.config(), app),
            SampleFormat::I16 => self.run::<i16>(&config.config(), app),
            SampleFormat::I32 => self.run::<i32>(&config.config(), app),
            SampleFormat::I64 => self.run::<i64>(&config.config(), app),
            SampleFormat::U8 => self.run::<u8>(&config.config(), app),
            SampleFormat::U16 => self.run::<u16>(&config.config(), app),
            SampleFormat::U32 => self.run::<u32>(&config.config(), app),
            SampleFormat::U64 => self.run::<u64>(&config.config(), app),
            SampleFormat::F32 => self.run::<f32>(&config.config(), app),
            SampleFormat::F64 => self.run::<f64>(&config.config(), app),
            sample_format => panic!("unsupported sample format '{sample_format}'"),
        }
    }

    fn run<T>(&self, config: &StreamConfig, app: Arc<Mutex<AppState>>) -> Result<()>
    where
        T: SizedSample + FromSample<f32> + 'static,
    {
        let channels = config.channels as usize;
        let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

        let rx = self.rx.recv();
        let stream = self.device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                if let Ok(received_keypress) = &rx {
                    let app = app.lock().unwrap();
                    if let Some(audio) = app.get_audio_data(&received_keypress) {
                        let mut reader = Reader::new(Cursor::new(audio)).unwrap();
                        let samples: Vec<f32> = reader.samples().filter_map(Result::ok).collect();
                        let mut sample_iter = samples.into_iter().cycle();

                        write_data(data, channels, &mut || sample_iter.next().unwrap_or(0.0));
                    }
                }
            },
            err_fn,
            None,
        )?;

        stream.play()?;
        Ok(())
    }
}
fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: Sample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels) {
        let value: T = T::from_sample(next_sample());
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum SoundType {
    Backspace,
    Enter,
    Generic,
    Space,
}

#[derive(Clone, Debug, PartialEq)]
enum KeyPressType {
    Press,
    Release,
}

#[derive(Clone, Debug, PartialEq)]
struct KeyboardButtonSound {
    sound_type: SoundType,
    data: Vec<u8>,
}

impl KeyboardButtonSound {
    fn new(sound_type: SoundType, data: Vec<u8>) -> Self {
        Self { sound_type, data }
    }
}

struct AppState {
    theme: Theme,
    audio_press: Vec<KeyboardButtonSound>,
    audio_release: Vec<KeyboardButtonSound>,
}

impl AppState {
    fn new(theme: Theme) -> Self {
        Self {
            theme,
            audio_press: Vec::new(),
            audio_release: Vec::new(),
        }
    }

    fn load_audio_samples(&mut self) -> Result<()> {
        match self.theme {
            Theme::CherryMXBrown => {
                for audio in AUDIOFILE {
                    self.load_audio_on_memory(&audio, KeyPressType::Release)?;
                    self.load_audio_on_memory(&audio, KeyPressType::Press)?;
                }
                Ok(())
            }
        }
    }

    fn load_audio_on_memory(
        &mut self,
        audio: &(&str, SoundType),
        keypress: KeyPressType,
    ) -> Result<()> {
        let dir = match keypress {
            KeyPressType::Press => "press",
            KeyPressType::Release => "release",
        };

        let filepath = format!("{}/{}/{}/{}", ASSETS, CHERRYMXBROWN, dir, audio.0);
        let path_buf = PathBuf::from(filepath);

        if let Ok(f) = File::open(&path_buf) {
            let mut buffer = Vec::new();
            let mut file = BufReader::new(f);
            file.read_to_end(&mut buffer)?;
            let sound = KeyboardButtonSound::new(audio.1.clone(), buffer);
            if dir == "press" {
                self.audio_press.push(sound);
            } else {
                self.audio_release.push(sound);
            }
        }
        Ok(())
    }

    fn get_audio_data(&self, keypress: &(KeyPressType, SoundType)) -> Option<Vec<u8>> {
        let sounds = match keypress.0 {
            KeyPressType::Press => &self.audio_press,
            KeyPressType::Release => &self.audio_release,
        };
        sounds
            .iter()
            .find(|item| item.sound_type == keypress.1)
            .map(|item| item.data.clone())
    }
}

fn main() -> Result<()> {
    let (tx, rx) = mpsc::channel::<(KeyPressType, SoundType)>();

    let host = cpal::default_host();
    let app = Arc::new(Mutex::new(AppState::new(Theme::CherryMXBrown)));

    let cloned_app = Arc::clone(&app);
    let audio_load_handler = thread::spawn(move || {
        load_and_handle_audio(cloned_app, host, rx);
    });

    let keyboard_thread_handler = thread::spawn(move || {
        handle_keyboard(tx);
    });

    keyboard_thread_handler.join().unwrap();
    audio_load_handler.join().unwrap();

    Ok(())
}

fn load_and_handle_audio(
    app: Arc<Mutex<AppState>>,
    host: cpal::Host,
    rx: Receiver<(KeyPressType, SoundType)>,
) {
    {
        let mut app = app.lock().unwrap();
        app.load_audio_samples().unwrap();
    }
    if let Some(device) = host.default_output_device() {
        let config = device.default_output_config().unwrap();
        let audio_handler = AudioHandler::new(host, device, config, rx);
        audio_handler.stream_on_press(app).unwrap();
    }
}

fn map_key_to_sound(key: &Keycode) -> SoundType {
    match key {
        Keycode::Backspace => SoundType::Backspace,
        Keycode::Enter => SoundType::Enter,
        Keycode::Space => SoundType::Space,
        _ => SoundType::Generic,
    }
}

fn handle_keyboard(tx: Sender<(KeyPressType, SoundType)>) {
    let device_state = DeviceState::new();
    let _guard_release = device_state.on_key_up({
        let tx = tx.clone();
        move |key| {
            let sound_type = map_key_to_sound(key);
            if let Err(err) = tx.send((KeyPressType::Release, sound_type)) {
                eprintln!("Failed to send key release event: {:?}", err.to_string());
            }
        }
    });
    let _guard_down = device_state.on_key_down(move |key| {
        println!("Keyboard key down {:#?}", key);
        let sound_type = map_key_to_sound(key);
        if let Err(err) = tx.send((KeyPressType::Press, sound_type)) {
            eprintln!("Failed to send key press event: {:?}", err);
        }
    });
    loop {}
}
