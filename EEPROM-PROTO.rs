//! Just an idea for now

struct Eeprom {
    data: impl DataProvider;

    // The DataProvider impl here is a SlaveEepromIrl during normal operation, but could be a
    // file-backed EEPROM for unit testing. This allows the usage of the complete `Eeprom` struct
    // logic to e.g. parse types and stuff.
    pub fn new(data: impl DataProvider) {
        Self { data }
    }

    pub fn fmmus(&self) {
        reader = self.category(Category::Fmmu).await?;

        reader.take_vec_whatever();
    }

    fn start_at(&self, addr: u16, len: u16) {
        let mut r = self.data.reader();

        r.seek(addr);

        ChunkReader::new(r, len)
    }

    // Category search logic is moved here to reduce duplication in each impl.
    fn category(&self, cat: Category) {
        let mut reader = self.data.reader();

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
                    // break Ok(Some(reader.set_len(len_words * 2)));
                    break Ok(Some(ChunkReader::new(reader, len_words * 2)));
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

/// A sequential reader with a set length starting from a given address, allowing the reading of a
/// category.
///
/// Maybe AKA `CategoryReader`?
///
/// The internal `reader` handles partial chunk caching etc. This struct handles length limiting.
struct ChunkReader {
    reader: impl embedded_io_async::Read,
    /// Max number of bytes we're allowed to read
    len: u16,
    /// Current number of bytes we've read
    byte_count: usize,

    fn new(reader: impl Read, len_bytes: u16) -> Self {
        todo!()
    }

    // TODO: Is this actually needed as an API?
    // async fn next(&mut Self) -> u8 {
    //     todo!()
    // }

    async fn take_vec_exact(&mut self) {
        todo!()
    }

    // And all the other take_* methods
}

// ---

/// Holds e.g. a file path, or a slaveclient. Can create ChunkReader handles to allow "concurrent"
/// EEPROM reads.
///
/// This is only required so we can get multiple copies of e.g. the file path, or the slave client,
/// whilst abstracting over what exactly we're asking for.
trait DataProvider {
    fn reader(&self) -> ChunkReader;
}

// ---

// Handles reading of chunks, storing intermediate slices that haven't been asked, for, but does not
// handle e.g. category length limiting.
struct SlaveEepromIrl {
    client: SlaveClient,
    buf: heapless::Deque,
    // etc
}

impl embedded_io_async::Read for SlaveEepromIrl {
    fn read() {
        // buf, next, dequeue, etc
    }
}

// ---

// Handles reading of chunks, storing intermediate slices that haven't been asked, for, but does not
// handle e.g. category length limiting.
struct MockEeprom {
    path: PathBuf
}

impl embedded_io_async::Read for MockEeprom {
    fn read() {
        // buf, next, dequeue, etc
    }
}
