//! Derive macros for `theater-guest`.
//!
//! The one macro here is [`macro@State`], the blessed opt-in for in-module actor
//! state (see `docs/in-module-state.md` and the `theater-guest` crate docs).

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// Make a type an actor's managed, inspectable state.
///
/// `#[derive(State)]` on a type gives you:
///
/// - a module-global cell holding one value of the type (Theater actors are
///   single-threaded, so this is sound — see [`theater_guest::StateCell`]);
/// - associated accessors on the type — `set`, `is_set`, `with`, `with_mut` —
///   so you install the state once in `init` and mutate it in place; and
/// - an auto-generated `theater:simple/actor.get-state` export that serializes
///   the current state, so the runtime's `get-actor-state` can inspect the actor
///   (and replay + `get-state` can reconstruct any historical state).
///
/// Because it emits `get-state`, the type must be `Clone` and convertible into a
/// packr `Value` — derive `Clone` and packr's `GraphValue` alongside it:
///
/// ```ignore
/// use theater_guest::State;
/// use theater_guest::packr_guest::{export, GraphValue};
///
/// #[derive(Clone, GraphValue, State)]
/// struct Counter { count: u64 }
///
/// #[export(name = "theater:simple/actor.init")]
/// fn init() { Counter::set(Counter { count: 0 }); }
///
/// #[export(name = "theater:simple/message-server-client.handle-send")]
/// fn handle_send(_from: String, _msg: Vec<u8>) { Counter::with_mut(|c| c.count += 1); }
/// ```
///
/// An actor whose state is non-serializable (handles, resources) or that simply
/// doesn't want to be inspectable should use [`theater_guest::StateCell`]
/// directly instead — it holds anything and exports nothing, staying opaque.
///
/// There is **one** state cell per actor: derive `State` on a single type. A
/// second `#[derive(State)]` in the same module is a compile error (the
/// `get-state` export symbol would collide).
#[proc_macro_derive(State)]
pub fn derive_state(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    // Actor state is a single concrete type living in a module-global cell; a
    // generic type has no one concrete layout to hold there.
    if !input.generics.params.is_empty() {
        return syn::Error::new_spanned(
            &input.generics,
            "#[derive(State)] does not support generic types — actor state is a single concrete type",
        )
        .to_compile_error()
        .into();
    }

    let expanded = quote! {
        // The actor's single state cell. Fixed name so a second derive collides
        // here (and at the get-state export) rather than silently making two.
        #[allow(non_upper_case_globals)]
        static __THEATER_STATE_CELL: ::theater_guest::StateCell<#name> =
            ::theater_guest::StateCell::new();

        impl #name {
            /// Install this actor's state. Call once from `init`.
            pub fn set(value: #name) {
                __THEATER_STATE_CELL.set(value);
            }

            /// Whether the actor's state has been set yet.
            pub fn is_set() -> bool {
                __THEATER_STATE_CELL.is_set()
            }

            /// Borrow the actor's state immutably for the duration of `f`.
            ///
            /// Panics if called before `init` set the state.
            pub fn with<__StateR>(f: impl FnOnce(&#name) -> __StateR) -> __StateR {
                __THEATER_STATE_CELL.with(f)
            }

            /// Borrow the actor's state mutably for the duration of `f`.
            ///
            /// Panics if called before `init` set the state.
            pub fn with_mut<__StateR>(f: impl FnOnce(&mut #name) -> __StateR) -> __StateR {
                __THEATER_STATE_CELL.with_mut(f)
            }
        }

        /// Auto-generated `theater:simple/actor.get-state` export: serializes the
        /// current state so the runtime's `get-actor-state` can inspect it.
        #[::packr_guest::export(name = "theater:simple/actor.get-state")]
        fn __theater_get_state() -> ::packr_guest::Value {
            __THEATER_STATE_CELL.with(|__s| {
                ::core::convert::Into::into(::core::clone::Clone::clone(__s))
            })
        }
    };

    expanded.into()
}
