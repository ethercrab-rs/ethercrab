//! Just an idea for now

struct Eeprom {
    data: impl DataProvider

    pub fn new(data: impl DataProvider) {
        Self { data }
    }

    pub fn fmmus(&self) {
        reader = self.category(Category::Fmmu).await?;

        reader.take_vec_whatever();
    }

    fn category(&self, cat: Category) {
        let mut reader = self.data.reader()

        reader.seek(SII_FIRST_CATEGORY_START);

        loop {
            let mut header = [0u8; 4];

            reader.read_exact(&mut header);

            // The chunk is either 4 or 8 bytes long, so these unwraps should never fire.
            let category_type =
                CategoryType::from(u16::from_le_bytes(fmt::unwrap!(header[0..2].try_into())));
            let len_words = u16::from_le_bytes(fmt::unwrap!(header[2..4].try_into()));

            // Position after header
            // Done inside read_exact
            // start_word += 2;

            fmt::trace!(
                "Found category {:?}, data starts at {:#06x}, length {:#04x} ({}) bytes",
                category_type,
                start_word,
                len_words,
                len_words
            );

            match category_type {
                cat if cat == category => {
                    break Ok(Some(reader));
                }
                CategoryType::End => break Ok(None),
                _ => (),
            }

            // Next category starts after the current category's data
            reader.seek(len_words * 2);
        }
    }
}

// ---

trait DataProvider {
    type Reader: DataProviderInstance;

    // fn category(impl embedded_io_async::Read);
    // fn start_at(impl embedded_io_async::Read);
    fn reader() -> Self::Reader
}

// ---

// Extra trait on top of async::Read so we can have things like take_vec_exact, etc.
trait DataProviderInstance: embedded_io_async::Read {
    async fn take_vec_exact(&mut self) {
        // These can all be direct impls because all we need is async::Read methods

        todo!()
    }

    // etc
}

// ---

struct SlaveEepromIrl {
    client: SlaveClient
    buf: heapless::Deque,
    // etc
}

impl embedded_io_async::Read for SlaveEepromIrl {
    fn read() {
        // buf, next, dequeue, etc
    }
}

// Could be blanket, but better to constrain to actual desired types
impl DataProviderInstance for SlaveEepromIrl {}

// ---

struct MockEeprom {
    path: PathBuf
}

impl embedded_io_async::Read for MockEeprom {
    fn read() {
        // buf, next, dequeue, etc
    }
}

// Could be blanket, but better to constrain to actual desired types
impl DataProviderInstance for SlaveEepromIrl {}
