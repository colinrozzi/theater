#[allow(dead_code)]
pub mod ntwk {
    #[allow(dead_code)]
    pub mod simple_actor {
        #[allow(dead_code, clippy::all)]
        pub mod types {
            #[used]
            #[doc(hidden)]
            static __FORCE_SECTION_REF: fn() = super::super::super::__link_custom_section_describing_imports;
            use super::super::super::_rt;
            /// Generic message type as bytes that can be serialized/deserialized
            pub type Message = _rt::Vec<u8>;
        }
        #[allow(dead_code, clippy::all)]
        pub mod runtime {
            #[used]
            #[doc(hidden)]
            static __FORCE_SECTION_REF: fn() = super::super::super::__link_custom_section_describing_imports;
            pub type Message = super::super::super::ntwk::simple_actor::types::Message;
            #[allow(unused_unsafe, clippy::all)]
            pub fn log(msg: &str) {
                unsafe {
                    let vec0 = msg;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "ntwk:simple-actor/runtime")]
                    extern "C" {
                        #[link_name = "log"]
                        fn wit_import(_: *mut u8, _: usize);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0);
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn send(actor_id: &str, msg: &Message) {
                unsafe {
                    let vec0 = actor_id;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let vec1 = msg;
                    let ptr1 = vec1.as_ptr().cast::<u8>();
                    let len1 = vec1.len();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "ntwk:simple-actor/runtime")]
                    extern "C" {
                        #[link_name = "send"]
                        fn wit_import(_: *mut u8, _: usize, _: *mut u8, _: usize);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize, _: *mut u8, _: usize) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1.cast_mut(), len1);
                }
            }
        }
    }
}
#[allow(dead_code)]
pub mod exports {
    #[allow(dead_code)]
    pub mod ntwk {
        #[allow(dead_code)]
        pub mod simple_actor {
            #[allow(dead_code, clippy::all)]
            pub mod actor {
                #[used]
                #[doc(hidden)]
                static __FORCE_SECTION_REF: fn() = super::super::super::super::__link_custom_section_describing_imports;
                use super::super::super::super::_rt;
                pub type Message = super::super::super::super::ntwk::simple_actor::types::Message;
                pub type State = _rt::Vec<u8>;
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_state_contract_cabi<T: Guest>(
                    arg0: *mut u8,
                    arg1: usize,
                ) -> i32 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let len0 = arg1;
                    let result1 = T::state_contract(
                        _rt::Vec::from_raw_parts(arg0.cast(), len0, len0),
                    );
                    match result1 {
                        true => 1,
                        false => 0,
                    }
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_message_contract_cabi<T: Guest>(
                    arg0: *mut u8,
                    arg1: usize,
                    arg2: *mut u8,
                    arg3: usize,
                ) -> i32 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let len0 = arg1;
                    let len1 = arg3;
                    let result2 = T::message_contract(
                        _rt::Vec::from_raw_parts(arg0.cast(), len0, len0),
                        _rt::Vec::from_raw_parts(arg2.cast(), len1, len1),
                    );
                    match result2 {
                        true => 1,
                        false => 0,
                    }
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_handle_cabi<T: Guest>(
                    arg0: *mut u8,
                    arg1: usize,
                    arg2: *mut u8,
                    arg3: usize,
                ) -> *mut u8 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let len0 = arg1;
                    let len1 = arg3;
                    let result2 = T::handle(
                        _rt::Vec::from_raw_parts(arg0.cast(), len0, len0),
                        _rt::Vec::from_raw_parts(arg2.cast(), len1, len1),
                    );
                    let ptr3 = _RET_AREA.0.as_mut_ptr().cast::<u8>();
                    let vec4 = (result2).into_boxed_slice();
                    let ptr4 = vec4.as_ptr().cast::<u8>();
                    let len4 = vec4.len();
                    ::core::mem::forget(vec4);
                    *ptr3.add(4).cast::<usize>() = len4;
                    *ptr3.add(0).cast::<*mut u8>() = ptr4.cast_mut();
                    ptr3
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn __post_return_handle<T: Guest>(arg0: *mut u8) {
                    let l0 = *arg0.add(0).cast::<*mut u8>();
                    let l1 = *arg0.add(4).cast::<usize>();
                    let base2 = l0;
                    let len2 = l1;
                    _rt::cabi_dealloc(base2, len2 * 1, 1);
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_init_cabi<T: Guest>() -> *mut u8 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let result0 = T::init();
                    let ptr1 = _RET_AREA.0.as_mut_ptr().cast::<u8>();
                    let vec2 = (result0).into_boxed_slice();
                    let ptr2 = vec2.as_ptr().cast::<u8>();
                    let len2 = vec2.len();
                    ::core::mem::forget(vec2);
                    *ptr1.add(4).cast::<usize>() = len2;
                    *ptr1.add(0).cast::<*mut u8>() = ptr2.cast_mut();
                    ptr1
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn __post_return_init<T: Guest>(arg0: *mut u8) {
                    let l0 = *arg0.add(0).cast::<*mut u8>();
                    let l1 = *arg0.add(4).cast::<usize>();
                    let base2 = l0;
                    let len2 = l1;
                    _rt::cabi_dealloc(base2, len2 * 1, 1);
                }
                pub trait Guest {
                    fn state_contract(state: State) -> bool;
                    fn message_contract(msg: Message, state: State) -> bool;
                    fn handle(msg: Message, state: State) -> State;
                    fn init() -> State;
                }
                #[doc(hidden)]
                macro_rules! __export_ntwk_simple_actor_actor_cabi {
                    ($ty:ident with_types_in $($path_to_types:tt)*) => {
                        const _ : () = { #[export_name =
                        "ntwk:simple-actor/actor#state-contract"] unsafe extern "C" fn
                        export_state_contract(arg0 : * mut u8, arg1 : usize,) -> i32 {
                        $($path_to_types)*:: _export_state_contract_cabi::<$ty > (arg0,
                        arg1) } #[export_name =
                        "ntwk:simple-actor/actor#message-contract"] unsafe extern "C" fn
                        export_message_contract(arg0 : * mut u8, arg1 : usize, arg2 : *
                        mut u8, arg3 : usize,) -> i32 { $($path_to_types)*::
                        _export_message_contract_cabi::<$ty > (arg0, arg1, arg2, arg3) }
                        #[export_name = "ntwk:simple-actor/actor#handle"] unsafe extern
                        "C" fn export_handle(arg0 : * mut u8, arg1 : usize, arg2 : * mut
                        u8, arg3 : usize,) -> * mut u8 { $($path_to_types)*::
                        _export_handle_cabi::<$ty > (arg0, arg1, arg2, arg3) }
                        #[export_name = "cabi_post_ntwk:simple-actor/actor#handle"]
                        unsafe extern "C" fn _post_return_handle(arg0 : * mut u8,) {
                        $($path_to_types)*:: __post_return_handle::<$ty > (arg0) }
                        #[export_name = "ntwk:simple-actor/actor#init"] unsafe extern "C"
                        fn export_init() -> * mut u8 { $($path_to_types)*::
                        _export_init_cabi::<$ty > () } #[export_name =
                        "cabi_post_ntwk:simple-actor/actor#init"] unsafe extern "C" fn
                        _post_return_init(arg0 : * mut u8,) { $($path_to_types)*::
                        __post_return_init::<$ty > (arg0) } };
                    };
                }
                #[doc(hidden)]
                pub(crate) use __export_ntwk_simple_actor_actor_cabi;
                #[repr(align(4))]
                struct _RetArea([::core::mem::MaybeUninit<u8>; 8]);
                static mut _RET_AREA: _RetArea = _RetArea(
                    [::core::mem::MaybeUninit::uninit(); 8],
                );
            }
        }
    }
}
mod _rt {
    pub use alloc_crate::vec::Vec;
    #[cfg(target_arch = "wasm32")]
    pub fn run_ctors_once() {
        wit_bindgen_rt::run_ctors_once();
    }
    pub unsafe fn cabi_dealloc(ptr: *mut u8, size: usize, align: usize) {
        if size == 0 {
            return;
        }
        let layout = alloc::Layout::from_size_align_unchecked(size, align);
        alloc::dealloc(ptr, layout);
    }
    extern crate alloc as alloc_crate;
    pub use alloc_crate::alloc;
}
/// Generates `#[no_mangle]` functions to export the specified type as the
/// root implementation of all generated traits.
///
/// For more information see the documentation of `wit_bindgen::generate!`.
///
/// ```rust
/// # macro_rules! export{ ($($t:tt)*) => (); }
/// # trait Guest {}
/// struct MyType;
///
/// impl Guest for MyType {
///     // ...
/// }
///
/// export!(MyType);
/// ```
#[allow(unused_macros)]
#[doc(hidden)]
macro_rules! __export_first_actor_impl {
    ($ty:ident) => {
        self::export!($ty with_types_in self);
    };
    ($ty:ident with_types_in $($path_to_types_root:tt)*) => {
        $($path_to_types_root)*::
        exports::ntwk::simple_actor::actor::__export_ntwk_simple_actor_actor_cabi!($ty
        with_types_in $($path_to_types_root)*:: exports::ntwk::simple_actor::actor);
    };
}
#[doc(inline)]
pub(crate) use __export_first_actor_impl as export;
#[cfg(target_arch = "wasm32")]
#[link_section = "component-type:wit-bindgen:0.35.0:ntwk:simple-actor:first-actor:encoded world"]
#[doc(hidden)]
pub static __WIT_BINDGEN_COMPONENT_TYPE: [u8; 501] = *b"\
\0asm\x0d\0\x01\0\0\x19\x16wit-component-encoding\x04\0\x07\xf3\x02\x01A\x02\x01\
A\x07\x01B\x02\x01p}\x04\0\x07message\x03\0\0\x03\0\x17ntwk:simple-actor/types\x05\
\0\x02\x03\0\0\x07message\x01B\x06\x02\x03\x02\x01\x01\x04\0\x07message\x03\0\0\x01\
@\x01\x03msgs\x01\0\x04\0\x03log\x01\x02\x01@\x02\x08actor-ids\x03msg\x01\x01\0\x04\
\0\x04send\x01\x03\x03\0\x19ntwk:simple-actor/runtime\x05\x02\x01B\x0c\x02\x03\x02\
\x01\x01\x04\0\x07message\x03\0\0\x01p}\x04\0\x05state\x03\0\x02\x01@\x01\x05sta\
te\x03\0\x7f\x04\0\x0estate-contract\x01\x04\x01@\x02\x03msg\x01\x05state\x03\0\x7f\
\x04\0\x10message-contract\x01\x05\x01@\x02\x03msg\x01\x05state\x03\0\x03\x04\0\x06\
handle\x01\x06\x01@\0\0\x03\x04\0\x04init\x01\x07\x04\0\x17ntwk:simple-actor/act\
or\x05\x03\x04\0\x1dntwk:simple-actor/first-actor\x04\0\x0b\x11\x01\0\x0bfirst-a\
ctor\x03\0\0\0G\x09producers\x01\x0cprocessed-by\x02\x0dwit-component\x070.220.0\
\x10wit-bindgen-rust\x060.35.0";
#[inline(never)]
#[doc(hidden)]
pub fn __link_custom_section_describing_imports() {
    wit_bindgen_rt::maybe_link_cabi_realloc();
}
