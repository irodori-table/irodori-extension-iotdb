use byteorder::{LittleEndian, ReadBytesExt}; // 如果是小端序数据

fn main() {
    let data: Vec<u8> = vec![0x01, 0x02, 0x03, 0x04];

    let mut cursor = std::io::Cursor::new(data);

    // 从 Cursor 中读取 i32 值
    let result = cursor.read_i32::<LittleEndian>().unwrap();

    println!("Read i32 value: {}", result);
}
