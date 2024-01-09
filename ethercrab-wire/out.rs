#[cfg(test)]
#[rustc_test_marker = "complex_enum"]
pub const complex_enum: test::TestDescAndFn = test::TestDescAndFn {
    desc: test::TestDesc {
        name: test::StaticTestName("complex_enum"),
        ignore: false,
        ignore_message: ::core::option::Option::None,
        source_file: "ethercrab-wire/tests/irl.rs",
        start_line: 211usize,
        start_col: 4usize,
        end_line: 211usize,
        end_col: 16usize,
        compile_fail: false,
        no_run: false,
        should_panic: test::ShouldPanic::No,
        test_type: test::TestType::IntegrationTest,
    },
    testfn: test::StaticTestFn(|| test::assert_test_result(complex_enum())),
};
fn complex_enum() {
    #[wire(bytes = 1)]
    pub struct InitSdoFlags {
        #[wire(bits = 1)]
        pub size_indicator: bool,
        #[wire(bits = 1)]
        pub expedited_transfer: bool,
        #[wire(bits = 2)]
        pub size: u8,
        #[wire(bits = 1)]
        pub complete_access: bool,
        #[wire(bits = 3)]
        pub command: u8,
    }
    #[automatically_derived]
    impl ::core::clone::Clone for InitSdoFlags {
        #[inline]
        fn clone(&self) -> InitSdoFlags {
            let _: ::core::clone::AssertParamIsClone<bool>;
            let _: ::core::clone::AssertParamIsClone<u8>;
            *self
        }
    }
    #[automatically_derived]
    impl ::core::marker::Copy for InitSdoFlags {}
    #[automatically_derived]
    impl ::core::fmt::Debug for InitSdoFlags {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            ::core::fmt::Formatter::debug_struct_field5_finish(
                f,
                "InitSdoFlags",
                "size_indicator",
                &self.size_indicator,
                "expedited_transfer",
                &self.expedited_transfer,
                "size",
                &self.size,
                "complete_access",
                &self.complete_access,
                "command",
                &&self.command,
            )
        }
    }
    #[automatically_derived]
    impl ::core::marker::StructuralPartialEq for InitSdoFlags {}
    #[automatically_derived]
    impl ::core::cmp::PartialEq for InitSdoFlags {
        #[inline]
        fn eq(&self, other: &InitSdoFlags) -> bool {
            self.size_indicator == other.size_indicator
                && self.expedited_transfer == other.expedited_transfer
                && self.size == other.size
                && self.complete_access == other.complete_access
                && self.command == other.command
        }
    }
    #[automatically_derived]
    impl ::core::marker::StructuralEq for InitSdoFlags {}
    #[automatically_derived]
    impl ::core::cmp::Eq for InitSdoFlags {
        #[inline]
        #[doc(hidden)]
        #[coverage(off)]
        fn assert_receiver_is_total_eq(&self) -> () {
            let _: ::core::cmp::AssertParamIsEq<bool>;
            let _: ::core::cmp::AssertParamIsEq<u8>;
        }
    }
    impl ::ethercrab_wire::EtherCrabWireWrite for InitSdoFlags {
        fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
            let buf = match buf.get_mut(0..1usize) {
                Some(buf) => buf,
                None => {
                    ::core::panicking::panic("internal error: entered unreachable code")
                }
            };
            buf[0usize] |= ((self.size_indicator as u8) << 0usize) & 0b00000001;
            buf[0usize] |= ((self.expedited_transfer as u8) << 1usize) & 0b00000010;
            buf[0usize] |= ((self.size as u8) << 2usize) & 0b00001100;
            buf[0usize] |= ((self.complete_access as u8) << 4usize) & 0b00010000;
            buf[0usize] |= ((self.command as u8) << 5usize) & 0b11100000;
            buf
        }
        fn packed_len(&self) -> usize {
            1usize
        }
    }
    impl ::ethercrab_wire::EtherCrabWireWriteSized for InitSdoFlags {
        fn pack(&self) -> Self::Buffer {
            let mut buf = [0u8; 1usize];
            <Self as ::ethercrab_wire::EtherCrabWireWrite>::pack_to_slice_unchecked(
                self,
                &mut buf,
            );
            buf
        }
    }
    impl ::ethercrab_wire::EtherCrabWireRead for InitSdoFlags {
        fn unpack_from_slice(buf: &[u8]) -> Result<Self, ::ethercrab_wire::WireError> {
            let buf = buf
                .get(0..1usize)
                .ok_or(::ethercrab_wire::WireError::ReadBufferTooShort {
                    expected: 1usize,
                    got: buf.len(),
                })?;
            Ok(Self {
                size_indicator: ((buf[0usize] & 0b00000001) >> 0usize) > 0,
                expedited_transfer: ((buf[0usize] & 0b00000010) >> 1usize) > 0,
                size: (buf[0usize] & 0b00001100) >> 2usize,
                complete_access: ((buf[0usize] & 0b00010000) >> 4usize) > 0,
                command: (buf[0usize] & 0b11100000) >> 5usize,
            })
        }
    }
    impl ::ethercrab_wire::EtherCrabWireSized for InitSdoFlags {
        const PACKED_LEN: usize = 1usize;
        type Buffer = [u8; 1usize];
        fn buffer() -> Self::Buffer {
            [0u8; 1usize]
        }
    }
    enum SdoHeader {
        #[wire(bytes = 4)]
        Normal {
            #[wire(bytes = 1)]
            flags: InitSdoFlags,
            #[wire(bytes = 2)]
            index: u16,
            #[wire(bytes = 1)]
            sub_index: u8,
        },
        #[wire(bytes = 1)]
        Segmented {
            #[wire(bits = 1)]
            is_last_segment: bool,
            /// Segment data size, `0x00` to `0x07`.
            #[wire(bits = 3)]
            segment_data_size: u8,
            #[wire(bits = 1)]
            toggle: bool,
            #[wire(bits = 3)]
            command: u8,
        },
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for SdoHeader {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            match self {
                SdoHeader::Normal {
                    flags: __self_0,
                    index: __self_1,
                    sub_index: __self_2,
                } => {
                    ::core::fmt::Formatter::debug_struct_field3_finish(
                        f,
                        "Normal",
                        "flags",
                        __self_0,
                        "index",
                        __self_1,
                        "sub_index",
                        &__self_2,
                    )
                }
                SdoHeader::Segmented {
                    is_last_segment: __self_0,
                    segment_data_size: __self_1,
                    toggle: __self_2,
                    command: __self_3,
                } => {
                    ::core::fmt::Formatter::debug_struct_field4_finish(
                        f,
                        "Segmented",
                        "is_last_segment",
                        __self_0,
                        "segment_data_size",
                        __self_1,
                        "toggle",
                        __self_2,
                        "command",
                        &__self_3,
                    )
                }
            }
        }
    }
    #[automatically_derived]
    impl ::core::marker::Copy for SdoHeader {}
    #[automatically_derived]
    impl ::core::clone::Clone for SdoHeader {
        #[inline]
        fn clone(&self) -> SdoHeader {
            let _: ::core::clone::AssertParamIsClone<InitSdoFlags>;
            let _: ::core::clone::AssertParamIsClone<u16>;
            let _: ::core::clone::AssertParamIsClone<u8>;
            let _: ::core::clone::AssertParamIsClone<bool>;
            *self
        }
    }
    #[automatically_derived]
    impl ::core::marker::StructuralPartialEq for SdoHeader {}
    #[automatically_derived]
    impl ::core::cmp::PartialEq for SdoHeader {
        #[inline]
        fn eq(&self, other: &SdoHeader) -> bool {
            let __self_tag = ::core::intrinsics::discriminant_value(self);
            let __arg1_tag = ::core::intrinsics::discriminant_value(other);
            __self_tag == __arg1_tag
                && match (self, other) {
                    (
                        SdoHeader::Normal {
                            flags: __self_0,
                            index: __self_1,
                            sub_index: __self_2,
                        },
                        SdoHeader::Normal {
                            flags: __arg1_0,
                            index: __arg1_1,
                            sub_index: __arg1_2,
                        },
                    ) => {
                        *__self_0 == *__arg1_0 && *__self_1 == *__arg1_1
                            && *__self_2 == *__arg1_2
                    }
                    (
                        SdoHeader::Segmented {
                            is_last_segment: __self_0,
                            segment_data_size: __self_1,
                            toggle: __self_2,
                            command: __self_3,
                        },
                        SdoHeader::Segmented {
                            is_last_segment: __arg1_0,
                            segment_data_size: __arg1_1,
                            toggle: __arg1_2,
                            command: __arg1_3,
                        },
                    ) => {
                        *__self_0 == *__arg1_0 && *__self_1 == *__arg1_1
                            && *__self_2 == *__arg1_2 && *__self_3 == *__arg1_3
                    }
                    _ => unsafe { ::core::intrinsics::unreachable() }
                }
        }
    }
}
