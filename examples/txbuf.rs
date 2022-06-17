// //! An experiment in putting PDUs straight into the TX buffer.

// use cookie_factory::bytes as make;
// use cookie_factory::gen_simple;
// use cookie_factory::GenError;
// use core::mem;
// use core::ops::Range;
// use ethercrab::pdu2::CommandCode;
// use ethercrab::pdu2::PduFlags;
// use packed_struct::PackedStruct;

// struct Ecat {
//     send_buf: [u8; 1024],
//     send_pdus: [Option<Range<usize>>; 16],

//     send_idx: u8,
//     send_start: usize,
// }

// impl Ecat {
//     pub fn brd<T>(&mut self, register_address: u16) -> Result<(), GenError> {
//         // Length of data field in the PDU
//         let data_len = mem::size_of::<T>();

//         // Length in bytes to store this PDU
//         let pdu_buf_len = data_len + 12;
//         let buf_end = self.send_start + pdu_buf_len;

//         let mut buf_range = self.send_start..(buf_end);

//         let mut buf = self
//             .send_buf
//             .get_mut(buf_range)
//             .expect("Not enough buf left");

//         let send_idx = {
//             // TODO: Clever auto increment index, error on full PDU slots

//             0
//         };

//         let flags = PduFlags::with_len(data_len.try_into().expect("Too long"));

//         // Order is VITAL here
//         let buf = gen_simple(make::le_u8(CommandCode::Brd as u8), buf)?;
//         let buf = gen_simple(make::le_u8(send_idx), buf)?;
//         // Autoincrement address, always zero when sending
//         let buf = gen_simple(make::le_u16(0), buf)?;
//         let buf = gen_simple(make::le_u16(register_address), buf)?;
//         let buf = gen_simple(
//             make::le_u16(u16::from_le_bytes(flags.pack().expect("Flag pack"))),
//             buf,
//         )?;
//         // IRQ is always zero on send
//         let buf = gen_simple(make::le_u16(0x0000), buf)?;
//         // Data is always zero on send
//         let buf = gen_simple(
//             cookie_factory::multi::all(core::iter::repeat(0x00u8).take(data_len).map(make::le_u8)),
//             buf,
//         )?;
//         // Working counter is always zero when sending
//         let buf = gen_simple(make::le_u16(0u16), buf)?;

//         Ok(())
//     }
// }

// fn main() {
//     //
// }
