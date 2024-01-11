# `ethercrab-wire`

[![Build Status](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master.svg?style=shield)](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master)
[![Crates.io](https://img.shields.io/crates/v/ethercrab-wire-derive.svg)](https://crates.io/crates/ethercrab-wire-derive)
[![Docs.rs](https://docs.rs/ethercrab-wire-derive/badge.svg)](https://docs.rs/ethercrab-wire-derive)

Derive attributes for [`ethercrab-wire`].

## Experimental

This crate is in its early stages and may contain bugs or publish breaking changes at any time.
It is in use by [`ethercrab`] and is well exercised there, but please use with caution in your
own code.

These derives support both structs with bit- and (multi)byte-sized fields for structs, as well
as enums with optional catch-all variant.

## Supported attributes

### Structs

- `#[wire(bits = N)]` OR `#[wire(bytes = N)]`

  The size of this struct when packed on the wire. These attributes may not be present at the
  same time.

### Struct fields

- `#[wire(bits = N)]` OR `#[wire(bytes = N)]`

  How many bytes this field consumes on the wire. These attributes may not be present at the
  same time.

- `#[wire(pre_skip = N)]` OR `#[wire(pre_skip_bytes = N)]`

  Skip one or more whole bytes before or after this field in the packed representation.

- `#[wire(post_skip = N)]` OR `#[wire(post_skip_bytes = N)]`

  How many bits or bytes to skip in the raw data **after** this field.

  These attributes are only applicable to fields that are less than 8 bits wide.

### Enums

Enums must have a `#[repr()]` attribute, as well as implement the `Copy` trait.

### Enum discriminants

Enum discriminants may not contain fields.

- `#[wire(alternatives = [])]`

  A discriminant with this attribute will be parsed successfully if either its direct value or
  any of the listed alternatives are found in the input data.

  The discriminant value is used when packing _to_ the wire.

- `#[wire(catch_all)]`

  Apply this once to a discriminant with a single unnamed field the same type as the enum's
  `#[repr()]` to catch any unrecognised values.

## Examples

### A struct with both bit fields and multi-byte fields.

```rust
#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 4)]
struct Mixed {
    #[wire(bits = 1)]
    one_bit: u8,
    #[wire(bits = 2)]
    two_bits: u8,

    // Fields that are 8 bits or larger must be byte aligned, so we skip the two remaining bits
    // of the previous byte with `post_skip`.
    #[wire(bits = 3, post_skip = 2)]
    three_bits: u8,

    /// Whole `u8`
    #[wire(bytes = 1)]
    one_byte: u8,

    /// Whole `u16`
    #[wire(bytes = 2)]
    one_word: u16,
}
```

### Enum with catch all discriminant and alternatives

```rust
#[derive(Copy, Clone, ethercrab_wire::EtherCrabWireReadWrite)]
#[repr(u8)]
enum OneByte {
    Foo = 0x01,
    #[wire(alternatives = [ 3, 4, 5, 6 ])]
    Bar = 0x02,
    Baz = 0x07,
    Quux = 0xab,
    #[wire(catch_all)]
    Unknown(u8),
}

// Normal discriminant
assert_eq!(OneByte::unpack_from_slice(&[0x07]), Ok(OneByte::Baz));

// Alternative value for `Bar`
assert_eq!(OneByte::unpack_from_slice(&[0x05]), Ok(OneByte::Bar));

// Catch all
assert_eq!(OneByte::unpack_from_slice(&[0xaa]), Ok(OneByte::Unknown(0xaa)));
```

## Struct field alignment

Struct fields of 1 byte or more MUST be byte-aligned. For example, the following struct will be
rejected due to `bar` being 5 bits "early":

```rust,compile_fail
#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 2)]
struct Broken {
    #[wire(bits = 3)]
    foo: u8,

    // There are 5 bits here unaccounted for

    #[wire(bytes = 1)]
    bar: u8,
}
```

This can easily be fixed by using the `pre_skip` or `post_skip` attributes to realign the next
field to 8 bits (or skip whole bytes of the input data):

```rust
#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 2)]
struct Fixed {
    #[wire(bits = 3, post_skip = 5)]
    foo: u8,
    #[wire(bytes = 1)]
    bar: u8,
}
```

A field in the middle of a byte can be written as such, maintaining 8 bit alignment:

```rust
#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 1)]
struct Middle {
    #[wire(pre_skip = 2, bits = 3, post_skip = 3)]
    foo: u8,
}
```

[`ethercrab`]: https://docs.rs/ethercrab
[`ethercrab-wire`]: https://docs.rs/ethercrab-wire

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
