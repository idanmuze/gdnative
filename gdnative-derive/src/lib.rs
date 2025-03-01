extern crate proc_macro;
extern crate proc_macro2;
#[macro_use]
extern crate syn;
#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::{AttributeArgs, DeriveInput, ItemFn, ItemImpl};

mod extend_bounds;
mod methods;
mod native_script;
mod profiled;
mod varargs;
mod variant;

/// Collects method signatures of all functions in a `NativeClass` that have the `#[method]` attribute and registers them with Godot.
///
/// For example, in the following class
/// ```
/// use gdnative::prelude::*;
///
/// #[derive(NativeClass)]
/// #[inherit(Reference)]
/// #[no_constructor]
/// struct Foo {}
///
/// #[methods]
/// impl Foo {
///     #[method]
///     fn foo(&self, #[base] _base: &Reference, bar: i64) -> i64 {
///         bar
///     }
/// }
///
/// ```
/// Will expand to
/// ```
/// use gdnative::prelude::*;
/// struct Foo {}
/// impl NativeClass for Foo {
///     type Base = gdnative::api::Reference;
///     type UserData = gdnative::export::user_data::LocalCellData<Self>;
/// }
/// impl gdnative::export::StaticallyNamed for Foo {
///     const CLASS_NAME: &'static str = "Foo";
/// }
/// impl gdnative::export::NativeClassMethods for Foo {
///     fn nativeclass_register(builder: &ClassBuilder<Self>) {
///         use gdnative::export::*;
///         builder.method("foo", gdnative::export::godot_wrap_method!(Foo, false, fn foo(&self, #[base] _base: &Reference, bar: i64) -> i64))
///             .with_rpc_mode(RpcMode::Disabled)
///             .done_stateless();
///     }
/// }
/// impl Foo {
///     fn foo(&self, _owner: &Reference, bar: i64) -> i64 {
///         bar
///     }
/// }
/// ```
/// **Important**: Only one `impl` block per struct may be attributed with `#[methods]`.
///
/// For more context, please refer to [gdnative::derive::NativeClass](NativeClass).
#[proc_macro_attribute]
pub fn methods(meta: TokenStream, input: TokenStream) -> TokenStream {
    if syn::parse::<syn::parse::Nothing>(meta.clone()).is_err() {
        let err = syn::Error::new_spanned(
            TokenStream2::from(meta),
            "#[methods] does not take parameters.",
        );
        return error_with_input(input, err);
    }

    let impl_block = match syn::parse::<ItemImpl>(input.clone()) {
        Ok(impl_block) => impl_block,
        Err(err) => return error_with_input(input, err),
    };

    fn error_with_input(input: TokenStream, err: syn::Error) -> TokenStream {
        let mut err = TokenStream::from(err.to_compile_error());
        err.extend(std::iter::once(input));
        err
    }

    TokenStream::from(methods::derive_methods(impl_block))
}

/// Makes a function profiled in Godot's built-in profiler. This macro automatically
/// creates a tag using the name of the current module and the function by default.
///
/// This attribute may also be used on non-exported functions. If the GDNative API isn't
/// initialized when the function is called, the data will be ignored silently.
///
/// A custom tag can also be provided using the `tag` option.
///
/// See the `gdnative::export::profiler` for a lower-level API to the profiler with
/// more control.
///
/// # Examples
///
/// ```ignore
/// mod foo {
///     // This function will show up as `foo/bar` under Script Functions.
///     #[profiled]
///     fn bar() {
///         std::thread::sleep(std::time::Duration::from_millis(1));
///     }
/// }
/// ```
///
/// ```ignore
/// // This function will show up as `my_custom_tag` under Script Functions.
/// #[profiled(tag = "my_custom_tag")]
/// fn baz() {
///     std::thread::sleep(std::time::Duration::from_millis(1));
/// }
/// ```
#[proc_macro_attribute]
pub fn profiled(meta: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(meta as AttributeArgs);
    let item_fn = parse_macro_input!(input as ItemFn);

    match profiled::derive_profiled(args, item_fn) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Makes it possible to use a type as a NativeScript.
///
/// ## Type attributes
///
/// The behavior of the derive macro can be customized using attributes on the type
/// deriving `NativeClass`. All type attributes are optional.
///
/// ### `#[inherit(gdnative::api::BaseClass)]`
///
/// Sets `gdnative::api::BaseClass` as the base class for the script. This *must* be
/// a type from the generated Godot API (that implements `GodotObject`). All `owner`
/// arguments of exported methods must be references (`TRef`, `Ref`, or `&`) to this
/// type.
///
/// Inheritance from other scripts, either in Rust or other languages, is
/// not supported.
///
/// If no `#[inherit(...)]` is provided, [`gdnative::api::Reference`](../gdnative/api/struct.Reference.html)
/// is used as a base class. This behavior is consistent with GDScript: omitting the
/// `extends` keyword will inherit `Reference`.
///
///
/// ### `#[user_data(gdnative::user_data::SomeWrapper<Self>)]`
///
/// Use the given type as the user-data wrapper. See the module-level docs on
/// `gdnative::user_data` for more information.
///
/// ### `#[register_with(path::to::function)]`
///
/// Use a custom function to register signals, properties or methods, in addition
/// to the one generated by `#[methods]`:
///
/// ```
/// use gdnative::prelude::*;
/// use gdnative::export::hint::{RangeHint, FloatHint};
///
/// #[derive(NativeClass)]
/// #[inherit(Reference)]
/// #[register_with(Self::my_register_function)]
/// struct Foo;
///
/// #[methods]
/// impl Foo {
///     fn new(_: &Reference) -> Self {
///         Self {}
///     }
///     fn my_register_function(builder: &ClassBuilder<Foo>) {
///         builder.signal("my_sig").done();
///         builder.property::<f32>("my_prop")
///             .with_getter(|_, _| 42.0)
///             .with_hint(FloatHint::Range(RangeHint::new(0.0, 100.0)))
///             .done();
///     }
/// }
/// ```
///
/// ### `#[no_constructor]`
///
/// Indicates that this type has no zero-argument constructor. Instances of such
/// scripts can only be created from Rust using `Instance::emplace`. `Instance::new`
/// or `ScriptName.new` from GDScript will result in panics at runtime.
///
/// See documentation on `Instance::emplace` for an example on how this can be used.
///
///
/// ## Field attributes
///
/// All field attributes are optional.
///
/// ### `#[property]`
///
/// Convenience attribute to register a field as a property. Possible arguments for
/// the attribute are:
///
/// - `path = "my_category/my_property_name"`
///
///   Puts the property under the `my_category` category and renames it to
///   `my_property_name` in the inspector and for GDScript.
///
/// - `default = 42.0`
///
///   Sets the default value *in the inspector* for this property. The setter is *not*
///   guaranteed to be called by the engine with the value.
///
/// - `get` / `get_ref` / `set`
///
///   Configure getter/setter for property. All of them can accept a path to specify a custom
///   property accessor. For example, `#[property(get = "Self::my_getter")]` will use
///   `Self::my_getter` as the getter.
///
///   The difference of `get` and `get_ref` is that `get` will register the getter with
///   `with_getter` function, which means your getter should return an owned value `T`, but
///   `get_ref` use `with_ref_getter` to register getter. In this case, your custom getter
///   should return a shared reference `&T`.
///
///   Situations with custom getters/setters and no backing fields require the use of the
///   type [`Property<T>`][gdnative::export::Property]. Consult its documentation for
///   a deeper elaboration of property exporting.
///
/// - `no_editor`
///
///   Hides the property from the editor. Does not prevent it from being sent over network or saved in storage.
///
/// ### `#[methods]`
/// Adds the necessary information to a an `impl` block to register the properties and methods with Godot.
///
/// **Important**: This needs to be added to one and only one `impl` block for a given `NativeClass`.
///
/// For additional details about how `#[methods]` expands, please refer to [gdnative::methods](macro@methods)
///
/// ### `#[method]`
/// Registers the attributed function signature to be used by Godot.
///
/// This attribute was formerly called `#[export]`, but is not directly related to the concept of
/// [exporting](https://docs.godotengine.org/en/stable/tutorials/export/exporting_basics.html) in GDScript.
///
/// A valid function signature must have:
/// - `&self` or `&mut self` as its first parameter
/// - Optionally, `&T` or `TRef<T>` where T refers to the type declared in `#[inherit(T)]` attribute as it's second parameter;
///   this is typically called the _base_. The parameter must be attributed with `#[base]`.
/// - Any number of required parameters, which must have the type `Variant` or must implement the `FromVariant` trait.
///  `FromVariant` is implemented for most common types.
/// - Any number of optional parameters annotated with `#[opt]`. Same rules as for required parameters apply.
///   Optional parameters must appear at the end of the parameter list.
/// - Return values must implement the `OwnedToVariant` trait (automatically implemented by `ToVariant`)
///   or be a `Variant` type.
///
/// ```ignore
/// // No access to base parameter
/// #[method]
/// fn foo(&self);
///
/// // Access base parameter as &T
/// #[method]
/// fn foo(&self, #[base] base: &Reference);
///
/// // Access base parameter as TRef<T>
/// #[method]
/// fn foo(&self, #[base] base: TRef<Reference>);
/// ```
///
/// **Note**: Marking a function with `#[method]` does not have any effect unless inside an `impl` block that has the `#[methods]` attribute.
///
/// Possible arguments for this attribute are:
///
/// - `name = "overridden_function_name"`
///
///   Overrides the function name as the method name to be registered in Godot.
///
/// - `rpc = "selected_rpc"`
///
///   `"selected_rpc"` must be one of the following values, which refer to the associated [Multiplayer API RPC Mode](https://docs.godotengine.org/en/stable/classes/class_multiplayerapi.html?highlight=RPC#enumerations).
///   See also the Rust type [`export::RpcMode`].
///     - `"disabled"`
///     - `"remote"`
///     - `"remote_sync"`
///     - `"master"`
///     - `"master_sync"`
///     - `"puppet"`
///     - `"puppet_sync"`
///
///   This enables you to set the [Multiplayer API RPC Mode](https://docs.godotengine.org/en/stable/classes/class_multiplayerapi.html?highlight=RPC#enumerations) for the function.
///   Refer to [Godot's Remote Procedure documentation](https://docs.godotengine.org/en/stable/tutorials/networking/high_level_multiplayer.html#rpc) for more details.
///
/// - `deref_return`
///
///   Allows you to return a type using its `Deref` representation. This can avoid extra intermediate copies for larger objects, by explicitly
///   returning a reference (or in general, a type that dereferences to something that can be exported).
///
///   For example:
///   ```ignore
///   #[method(deref_return)]
///   fn get_numbers(&self) -> std::cell::Ref<Vec<i32>> {
///      // Assume self.cell is std::cell::RefCell<Vec<i32>>
///      self.cell.borrow()
///   }
///   ```
///
///
/// #### `Node` virtual functions
///
/// This is a list of common Godot virtual functions that are automatically called via [notifications](https://docs.godotengine.org/en/stable/classes/class_object.html#class-object-method-notification).
///
/// It is assumed that every method is exported via `#[method]` attribute. The parameter `#[base] base: &Node` can be omitted if you don't need it.
///
/// ```ignore
/// fn _ready(&self, #[base] base: &Node);
/// ```
/// Called when both the node and its children have entered the scene tree.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-method-ready) for more information._
/// <br><br>
///
/// ```ignore
/// fn _enter_tree(&self, #[base] base: &Node);
/// ```
/// Called when the node enters the scene tree.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-method-enter-tree) for more information._
/// <br><br>
///
/// ```ignore
/// fn _exit_tree(&self, #[base] base: &Node);
/// ```
/// Called when the node is removed from the scene tree.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-method-exit-tree) for more information._
/// <br><br>
///
/// ```ignore
/// fn _get_configuration_warning(&self, #[base] base: &Node) -> GodotString;
/// ```
/// The string returned from this method is displayed as a warning in the Scene Dock if the script that overrides it is a tool script.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-method-get-configuration-warning) for more information._
/// <br><br>
///
/// ```ignore
/// fn _process(&mut self, #[base] base: &Node, delta: f64);
/// ```
/// Called during processing step of the main loop.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-method-process) for more information._
/// <br><br>
///
/// ```ignore
/// fn _physics_process(&self, #[base] base: &Node, delta: f64);
/// ```
/// Called during physics update, with a fixed timestamp.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-method-physics-process) for more information._
/// <br><br>
///
/// ```ignore
/// fn _input(&self, #[base] base: &Node, event: Ref<InputEvent>);
/// ```
/// Called when there is an input event.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-method-input) for more information._
/// <br><br>
///
/// ```ignore
/// fn _unhandled_input(&self, #[base] base: &Node, event: Ref<InputEvent>);
/// ```
/// Called when an `InputEvent` hasn't been consumed by `_input()` or any GUI.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-method-unhandled-input) for more information._
/// <br><br>
///
/// ```ignore
/// fn _unhandled_key_input (&self, #[base] base: &Node, event: Ref<InputKeyEvent>);
/// ```
/// Called when an `InputEventKey` hasn't been consumed by `_input()` or any GUI.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-method-unhandled-key-input) for more information._
/// <br><br>
///
/// #### `Control` virtual functions
///
/// This is a list of common Godot virtual functions that are automatically called via [notifications](https://docs.godotengine.org/en/stable/classes/class_object.html#class-object-method-notification).
///
/// ```ignore
/// fn _clips_input(&self, #[base] base: &Control) -> bool;
/// ```
/// Returns whether `_gui_input()` should not be called for children controls outside this control's rectangle.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_control.html#class-control-method-clips-input) for more information._
/// <br><br>
///
/// ```ignore
/// fn _get_minimum_size(&self, #[base] base: &Control) -> Vector2;
/// ```
/// Returns the minimum size for this control.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_control.html#class-control-method-get-minimum-size) for more information._
/// <br><br>
///
/// ```ignore
/// fn _gui_input(&self, #[base] base: &Control, event: Ref<InputEvent>);
/// ```
/// Use this method to process and accept inputs on UI elements.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_control.html#class-control-method-gui-input) for more information._
/// <br><br>
///
/// ```ignore
/// fn _make_custom_tooltip(&self, #[base] base: &Control, for_text: String) -> Ref<Control>;
/// ```
/// Returns a `Control` node that should be used as a tooltip instead of the default one.  
/// _See [Godot docs](https://docs.godotengine.org/en/stable/classes/class_control.html#class-control-method-make-custom-tooltip) for more information._
/// <br><br>
#[proc_macro_derive(
    NativeClass,
    attributes(inherit, register_with, no_constructor, user_data, property)
)]
pub fn derive_native_class(input: TokenStream) -> TokenStream {
    // Converting the proc_macro::TokenStream into non proc_macro types so that tests
    // can be written against the inner functions.
    let derive_input = syn::parse_macro_input!(input as DeriveInput);

    // Implement NativeClass for the input
    let derived = native_script::derive_native_class(&derive_input).map_or_else(
        |err| {
            // Silence the other errors that happen because NativeClass is not implemented
            let empty_nativeclass = native_script::impl_empty_nativeclass(&derive_input);
            let err = err.to_compile_error();

            quote! {
                #empty_nativeclass
                #err
            }
        },
        std::convert::identity,
    );

    TokenStream::from(derived)
}

#[proc_macro_derive(ToVariant, attributes(variant))]
pub fn derive_to_variant(input: TokenStream) -> TokenStream {
    match variant::derive_to_variant(variant::ToVariantTrait::ToVariant, input) {
        Ok(stream) => stream.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_derive(OwnedToVariant, attributes(variant))]
pub fn derive_owned_to_variant(input: TokenStream) -> TokenStream {
    match variant::derive_to_variant(variant::ToVariantTrait::OwnedToVariant, input) {
        Ok(stream) => stream.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_derive(FromVariant, attributes(variant))]
pub fn derive_from_variant(input: TokenStream) -> TokenStream {
    let derive_input = syn::parse_macro_input!(input as syn::DeriveInput);
    match variant::derive_from_variant(derive_input) {
        Ok(stream) => stream.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Enable struct types to be parsed as argument lists.
///
/// The `FromVarargs` trait can be derived for structure types where each type implements
/// `FromVariant`. The order of fields matter for this purpose:
///
/// ```ignore
/// #[derive(FromVarargs)]
/// struct MyArgs {
///     foo: i32,
///     bar: String,
///     #[opt] baz: Option<Ref<Node>>,
/// }
/// ```
#[proc_macro_derive(FromVarargs, attributes(opt))]
pub fn derive_from_varargs(input: TokenStream) -> TokenStream {
    let derive_input = syn::parse_macro_input!(input as syn::DeriveInput);
    match varargs::derive_from_varargs(derive_input) {
        Ok(stream) => stream.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Returns a standard header for derived implementations.
///
/// Adds the `automatically_derived` attribute and prevents common lints from triggering
/// in user code. See:
///
/// - https://doc.rust-lang.org/reference/attributes/derive.html
/// - https://doc.rust-lang.org/rustc/lints/groups.html
/// - https://github.com/rust-lang/rust-clippy#clippy
fn automatically_derived() -> proc_macro2::TokenStream {
    quote! {
        #[automatically_derived]
        #[allow(nonstandard_style, unused, clippy::style, clippy::complexity, clippy::perf, clippy::pedantic)]
    }
}
