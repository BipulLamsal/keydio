use anyhow::Result;
use device_query::{DeviceEvents, DeviceState, Keycode};
use rodio::{source::Source, Decoder, OutputStream, OutputStreamHandle};
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
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let app = Arc::new(Mutex::new(AppState::new(Theme::CherryMXBrown)));
    let cloned_app = Arc::clone(&app);
    let audio_load_handler = thread::spawn(move || {
        load_and_handle_audio(cloned_app, stream_handle, rx);
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
    stream_handle: OutputStreamHandle,
    rx: Receiver<(KeyPressType, SoundType)>,
) {
    let mut app = app.lock().unwrap();
    app.load_audio_samples().unwrap();

    loop {
        match rx.recv() {
            Ok((key_press, sound_type)) => {
                if let Some(audio) = app.get_audio_data(&(key_press, sound_type)) {
                    if let Ok(source) = Decoder::new(Cursor::new(audio)) {
                        stream_handle.play_raw(source.convert_samples()).unwrap();
                    }
                }
            }
            Err(err) => {
                eprintln!("Error receiving message: {:?}", err);
                break;
            }
        }
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
        let sound_type = map_key_to_sound(key);
        if let Err(err) = tx.send((KeyPressType::Press, sound_type)) {
            eprintln!("Failed to send key press event: {:?}", err);
        }
    });
    loop {}
}
