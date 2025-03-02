/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The implementation of the DOM.
//!
//! The DOM is comprised of interfaces (defined by specifications using
//! [WebIDL](https://heycam.github.io/webidl/)) that are implemented as Rust
//! structs in submodules of this module. Its implementation is documented
//! below.
//!
//! A DOM object and its reflector
//! ==============================
//!
//! The implementation of an interface `Foo` in Servo's DOM involves two
//! related but distinct objects:
//!
//! * the **DOM object**: an instance of the Rust struct `dom::foo::Foo`
//!   (marked with the `#[dom_struct]` attribute) on the Rust heap;
//! * the **reflector**: a `JSObject` allocated by SpiderMonkey, that owns the
//!   DOM object.
//!
//! Memory management
//! =================
//!
//! Reflectors of DOM objects, and thus the DOM objects themselves, are managed
//! by the SpiderMonkey Garbage Collector. Thus, keeping alive a DOM object
//! is done through its reflector.
//!
//! For more information, see:
//!
//! * rooting pointers on the stack:
//!   the [`Root`](bindings/root/struct.Root.html) smart pointer;
//! * tracing pointers in member fields: the [`Dom`](bindings/root/struct.Dom.html),
//!   [`MutNullableDom`](bindings/root/struct.MutNullableDom.html) and
//!   [`MutDom`](bindings/root/struct.MutDom.html) smart pointers and
//!   [the tracing implementation](bindings/trace/index.html);
//! * rooting pointers from across thread boundaries or in channels: the
//!   [`Trusted`](bindings/refcounted/struct.Trusted.html) smart pointer;
//!
//! Inheritance
//! ===========
//!
//! Rust does not support struct inheritance, as would be used for the
//! object-oriented DOM APIs. To work around this issue, Servo stores an
//! instance of the superclass in the first field of its subclasses. (Note that
//! it is stored by value, rather than in a smart pointer such as `Dom<T>`.)
//!
//! This implies that a pointer to an object can safely be cast to a pointer
//! to all its classes.
//!
//! This invariant is enforced by the lint in
//! `plugins::lints::inheritance_integrity`.
//!
//! Interfaces which either derive from or are derived by other interfaces
//! implement the `Castable` trait, which provides three methods `is::<T>()`,
//! `downcast::<T>()` and `upcast::<T>()` to cast across the type hierarchy
//! and check whether a given instance is of a given type.
//!
//! ```ignore
//! use dom::bindings::inheritance::Castable;
//! use dom::element::Element;
//! use dom::htmlelement::HTMLElement;
//! use dom::htmlinputelement::HTMLInputElement;
//!
//! if let Some(elem) = node.downcast::<Element> {
//!     if elem.is::<HTMLInputElement>() {
//!         return elem.upcast::<HTMLElement>();
//!     }
//! }
//! ```
//!
//! Furthermore, when discriminating a given instance against multiple
//! interface types, code generation provides a convenient TypeId enum
//! which can be used to write `match` expressions instead of multiple
//! calls to `Castable::is::<T>`. The `type_id()` method of an instance is
//! provided by the farthest interface it derives from, e.g. `EventTarget`
//! for `HTMLMediaElement`. For convenience, that method is also provided
//! on the `Node` interface to avoid unnecessary upcasts to `EventTarget`.
//!
//! ```ignore
//! use dom::bindings::inheritance::{EventTargetTypeId, NodeTypeId};
//!
//! match *node.type_id() {
//!     EventTargetTypeId::Node(NodeTypeId::CharacterData(_)) => ...,
//!     EventTargetTypeId::Node(NodeTypeId::Element(_)) => ...,
//!     ...,
//! }
//! ```
//!
//! Construction
//! ============
//!
//! DOM objects of type `T` in Servo have two constructors:
//!
//! * a `T::new_inherited` static method that returns a plain `T`, and
//! * a `T::new` static method that returns `DomRoot<T>`.
//!
//! (The result of either method can be wrapped in `Result`, if that is
//! appropriate for the type in question.)
//!
//! The latter calls the former, boxes the result, and creates a reflector
//! corresponding to it by calling `dom::bindings::utils::reflect_dom_object`
//! (which yields ownership of the object to the SpiderMonkey Garbage Collector).
//! This is the API to use when creating a DOM object.
//!
//! The former should only be called by the latter, and by subclasses'
//! `new_inherited` methods.
//!
//! DOM object constructors in JavaScript correspond to a `T::Constructor`
//! static method. This method is always fallible.
//!
//! Destruction
//! ===========
//!
//! When the SpiderMonkey Garbage Collector discovers that the reflector of a
//! DOM object is garbage, it calls the reflector's finalization hook. This
//! function deletes the reflector's DOM object, calling its destructor in the
//! process.
//!
//! Mutability and aliasing
//! =======================
//!
//! Reflectors are JavaScript objects, and as such can be freely aliased. As
//! Rust does not allow mutable aliasing, mutable borrows of DOM objects are
//! not allowed. In particular, any mutable fields use `Cell` or `DomRefCell`
//! to manage their mutability.
//!
//! `Reflector` and `DomObject`
//! =============================
//!
//! Every DOM object has a `Reflector` as its first (transitive) member field.
//! This contains a `*mut JSObject` that points to its reflector.
//!
//! The `FooBinding::Wrap` function creates the reflector, stores a pointer to
//! the DOM object in the reflector, and initializes the pointer to the reflector
//! in the `Reflector` field.
//!
//! The `DomObject` trait provides a `reflector()` method that returns the
//! DOM object's `Reflector`. It is implemented automatically for DOM structs
//! through the `#[dom_struct]` attribute.
//!
//! Implementing methods for a DOM object
//! =====================================
//!
//! * `dom::bindings::codegen::Bindings::FooBindings::FooMethods` for methods
//!   defined through IDL;
//! * `&self` public methods for public helpers;
//! * `&self` methods for private helpers.
//!
//! Accessing fields of a DOM object
//! ================================
//!
//! All fields of DOM objects are private; accessing them from outside their
//! module is done through explicit getter or setter methods.
//!
//! Inheritance and casting
//! =======================
//!
//! All DOM interfaces part of an inheritance chain (i.e. interfaces
//! that derive others or are derived from) implement the trait `Castable`
//! which provides both downcast and upcasts.
//!
//! ```ignore
//! # use script::dom::bindings::inheritance::Castable;
//! # use script::dom::element::Element;
//! # use script::dom::node::Node;
//! # use script::dom::htmlelement::HTMLElement;
//! fn f(element: &Element) {
//!     let base = element.upcast::<Node>();
//!     let derived = element.downcast::<HTMLElement>().unwrap();
//! }
//! ```
//!
//! Adding a new DOM interface
//! ==========================
//!
//! Adding a new interface `Foo` requires at least the following:
//!
//! * adding the new IDL file at `components/script/dom/webidls/Foo.webidl`;
//! * creating `components/script/dom/foo.rs`;
//! * listing `foo.rs` in `components/script/dom/mod.rs`;
//! * defining the DOM struct `Foo` with a `#[dom_struct]` attribute, a
//!   superclass or `Reflector` member, and other members as appropriate;
//! * implementing the
//!   `dom::bindings::codegen::Bindings::FooBindings::FooMethods` trait for
//!   `Foo`;
//! * adding/updating the match arm in create_element in
//!   `components/script/dom/create.rs` (only applicable to new types inheriting
//!   from `HTMLElement`)
//!
//! More information is available in the [bindings module](bindings/index.html).
//!
//! Accessing DOM objects from layout
//! =================================
//!
//! Layout code can access the DOM through the
//! [`LayoutDom`](bindings/root/struct.LayoutDom.html) smart pointer. This does not
//! keep the DOM object alive; we ensure that no DOM code (Garbage Collection
//! in particular) runs while the layout thread is accessing the DOM.
//!
//! Methods accessible to layout are implemented on `LayoutDom<Foo>` using
//! `LayoutFooHelpers` traits.

#[macro_use]
pub mod macros;

pub mod types {
    include!(concat!(env!("OUT_DIR"), "/InterfaceTypes.rs"));
}

pub mod abstractworker;
pub mod abstractworkerglobalscope;
pub mod activation;
pub mod analysernode;
pub mod animationevent;
pub mod attr;
pub mod audiobuffer;
pub mod audiobuffersourcenode;
pub mod audiocontext;
pub mod audiodestinationnode;
pub mod audiolistener;
pub mod audionode;
pub mod audioparam;
pub mod audioscheduledsourcenode;
pub mod audiotrack;
pub mod audiotracklist;
pub mod baseaudiocontext;
pub mod beforeunloadevent;
pub mod bindings;
pub mod biquadfilternode;
pub mod blob;
pub mod bluetooth;
pub mod bluetoothadvertisingevent;
pub mod bluetoothcharacteristicproperties;
pub mod bluetoothdevice;
pub mod bluetoothpermissionresult;
pub mod bluetoothremotegattcharacteristic;
pub mod bluetoothremotegattdescriptor;
pub mod bluetoothremotegattserver;
pub mod bluetoothremotegattservice;
pub mod bluetoothuuid;
pub mod broadcastchannel;
pub mod canvasgradient;
pub mod canvaspattern;
pub mod canvasrenderingcontext2d;
pub mod cdatasection;
pub mod channelmergernode;
pub mod channelsplitternode;
pub mod characterdata;
pub mod client;
pub mod closeevent;
pub mod comment;
pub mod compositionevent;
pub mod console;
pub mod constantsourcenode;
mod create;
pub mod crypto;
pub mod css;
pub mod cssconditionrule;
pub mod cssfontfacerule;
pub mod cssgroupingrule;
pub mod cssimportrule;
pub mod csskeyframerule;
pub mod csskeyframesrule;
pub mod cssmediarule;
pub mod cssnamespacerule;
pub mod cssrule;
pub mod cssrulelist;
pub mod cssstyledeclaration;
pub mod cssstylerule;
pub mod cssstylesheet;
pub mod cssstylevalue;
pub mod csssupportsrule;
pub mod customelementregistry;
pub mod customevent;
pub mod dedicatedworkerglobalscope;
pub mod dissimilaroriginlocation;
pub mod dissimilaroriginwindow;
pub mod document;
pub mod documentfragment;
pub mod documentorshadowroot;
pub mod documenttype;
pub mod domexception;
pub mod domimplementation;
pub mod dommatrix;
pub mod dommatrixreadonly;
pub mod domparser;
pub mod dompoint;
pub mod dompointreadonly;
pub mod domquad;
pub mod domrect;
pub mod domrectreadonly;
pub mod domstringlist;
pub mod domstringmap;
pub mod domtokenlist;
pub mod dynamicmoduleowner;
pub mod element;
pub mod errorevent;
pub mod event;
pub mod eventsource;
pub mod eventtarget;
pub mod extendableevent;
pub mod extendablemessageevent;
pub mod fakexrdevice;
pub mod fakexrinputcontroller;
pub mod file;
pub mod filelist;
pub mod filereader;
pub mod filereadersync;
pub mod focusevent;
pub mod formdata;
pub mod formdataevent;
pub mod gainnode;
pub mod gamepad;
pub mod gamepadbutton;
pub mod gamepadbuttonlist;
pub mod gamepadevent;
pub mod gamepadlist;
pub mod gamepadpose;
pub mod globalscope;
pub mod gpu;
pub mod gpuadapter;
pub mod gpuadapterinfo;
pub mod gpubindgroup;
pub mod gpubindgrouplayout;
pub mod gpubuffer;
pub mod gpubufferusage;
pub mod gpucanvascontext;
pub mod gpucolorwrite;
pub mod gpucommandbuffer;
pub mod gpucommandencoder;
pub mod gpucompilationinfo;
pub mod gpucompilationmessage;
pub mod gpucomputepassencoder;
pub mod gpucomputepipeline;
pub mod gpudevice;
pub mod gpudevicelostinfo;
pub mod gpumapmode;
pub mod gpuoutofmemoryerror;
pub mod gpupipelinelayout;
pub mod gpuqueryset;
pub mod gpuqueue;
pub mod gpurenderbundle;
pub mod gpurenderbundleencoder;
pub mod gpurenderpassencoder;
pub mod gpurenderpipeline;
pub mod gpusampler;
pub mod gpushadermodule;
pub mod gpushaderstage;
pub mod gpusupportedlimits;
pub mod gputexture;
pub mod gputextureusage;
pub mod gputextureview;
pub mod gpuuncapturederrorevent;
pub mod gpuvalidationerror;
pub mod hashchangeevent;
pub mod headers;
pub mod history;
pub mod htmlanchorelement;
pub mod htmlareaelement;
pub mod htmlaudioelement;
pub mod htmlbaseelement;
pub mod htmlbodyelement;
pub mod htmlbrelement;
pub mod htmlbuttonelement;
pub mod htmlcanvaselement;
pub mod htmlcollection;
pub mod htmldataelement;
pub mod htmldatalistelement;
pub mod htmldetailselement;
pub mod htmldialogelement;
pub mod htmldirectoryelement;
pub mod htmldivelement;
pub mod htmldlistelement;
pub mod htmlelement;
pub mod htmlembedelement;
pub mod htmlfieldsetelement;
pub mod htmlfontelement;
pub mod htmlformcontrolscollection;
pub mod htmlformelement;
pub mod htmlframeelement;
pub mod htmlframesetelement;
pub mod htmlheadelement;
pub mod htmlheadingelement;
pub mod htmlhrelement;
pub mod htmlhtmlelement;
pub mod htmliframeelement;
pub mod htmlimageelement;
pub mod htmlinputelement;
pub mod htmllabelelement;
pub mod htmllegendelement;
pub mod htmllielement;
pub mod htmllinkelement;
pub mod htmlmapelement;
pub mod htmlmediaelement;
pub mod htmlmenuelement;
pub mod htmlmetaelement;
pub mod htmlmeterelement;
pub mod htmlmodelement;
pub mod htmlobjectelement;
pub mod htmlolistelement;
pub mod htmloptgroupelement;
pub mod htmloptionelement;
pub mod htmloptionscollection;
pub mod htmloutputelement;
pub mod htmlparagraphelement;
pub mod htmlparamelement;
pub mod htmlpictureelement;
pub mod htmlpreelement;
pub mod htmlprogresselement;
pub mod htmlquoteelement;
pub mod htmlscriptelement;
pub mod htmlselectelement;
pub mod htmlsourceelement;
pub mod htmlspanelement;
pub mod htmlstyleelement;
pub mod htmltablecaptionelement;
pub mod htmltablecellelement;
pub mod htmltablecolelement;
pub mod htmltableelement;
pub mod htmltablerowelement;
pub mod htmltablesectionelement;
pub mod htmltemplateelement;
pub mod htmltextareaelement;
pub mod htmltimeelement;
pub mod htmltitleelement;
pub mod htmltrackelement;
pub mod htmlulistelement;
pub mod htmlunknownelement;
pub mod htmlvideoelement;
pub mod identityhub;
pub mod imagebitmap;
pub mod imagedata;
pub mod inputevent;
pub mod keyboardevent;
pub mod location;
pub mod mediadeviceinfo;
pub mod mediadevices;
pub mod mediaelementaudiosourcenode;
pub mod mediaerror;
pub mod mediafragmentparser;
pub mod medialist;
pub mod mediametadata;
pub mod mediaquerylist;
pub mod mediaquerylistevent;
pub mod mediasession;
pub mod mediastream;
pub mod mediastreamaudiodestinationnode;
pub mod mediastreamaudiosourcenode;
pub mod mediastreamtrack;
pub mod mediastreamtrackaudiosourcenode;
pub mod messagechannel;
pub mod messageevent;
pub mod messageport;
pub mod mimetype;
pub mod mimetypearray;
pub mod mouseevent;
pub mod mutationobserver;
pub mod mutationrecord;
pub mod namednodemap;
pub mod navigationpreloadmanager;
pub mod navigator;
pub mod navigatorinfo;
pub mod node;
pub mod nodeiterator;
pub mod nodelist;
pub mod offlineaudiocompletionevent;
pub mod offlineaudiocontext;
pub mod offscreencanvas;
pub mod offscreencanvasrenderingcontext2d;
pub mod oscillatornode;
pub mod pagetransitionevent;
pub mod paintrenderingcontext2d;
pub mod paintsize;
pub mod paintworkletglobalscope;
pub mod pannernode;
pub mod performance;
pub mod performanceentry;
pub mod performancemark;
pub mod performancemeasure;
pub mod performancenavigation;
pub mod performancenavigationtiming;
pub mod performanceobserver;
pub mod performanceobserverentrylist;
pub mod performancepainttiming;
pub mod performanceresourcetiming;
pub mod permissions;
pub mod permissionstatus;
pub mod plugin;
pub mod pluginarray;
pub mod popstateevent;
pub mod processinginstruction;
pub mod progressevent;
pub mod promise;
pub mod promisenativehandler;
pub mod promiserejectionevent;
pub mod radionodelist;
pub mod range;
pub mod raredata;
pub mod readablestream;
pub mod request;
pub mod response;
pub mod rtcdatachannel;
pub mod rtcdatachannelevent;
pub mod rtcerror;
pub mod rtcerrorevent;
pub mod rtcicecandidate;
pub mod rtcpeerconnection;
pub mod rtcpeerconnectioniceevent;
pub(crate) mod rtcrtpsender;
pub(crate) mod rtcrtptransceiver;
pub mod rtcsessiondescription;
pub mod rtctrackevent;
pub mod screen;
pub mod selection;
pub mod serviceworker;
pub mod serviceworkercontainer;
pub mod serviceworkerglobalscope;
pub mod serviceworkerregistration;
pub mod servoparser;
pub mod shadowroot;
pub mod stereopannernode;
pub mod storage;
pub mod storageevent;
pub mod stylepropertymapreadonly;
pub mod stylesheet;
pub mod stylesheetlist;
pub mod submitevent;
pub mod svgelement;
pub mod svggraphicselement;
pub mod svgsvgelement;
pub mod testbinding;
pub mod testbindingiterable;
pub mod testbindingmaplike;
pub mod testbindingpairiterable;
pub mod testbindingproxy;
pub mod testbindingsetlike;
pub mod testrunner;
pub mod testworklet;
pub mod testworkletglobalscope;
pub mod text;
pub mod textcontrol;
pub mod textdecoder;
pub mod textencoder;
pub mod textmetrics;
pub mod texttrack;
pub mod texttrackcue;
pub mod texttrackcuelist;
pub mod texttracklist;
pub mod timeranges;
pub mod touch;
pub mod touchevent;
pub mod touchlist;
pub mod trackevent;
pub mod transitionevent;
pub mod treewalker;
pub mod uievent;
pub mod url;
pub mod urlhelper;
pub mod urlsearchparams;
pub mod userscripts;
pub mod validation;
pub mod validitystate;
pub mod values;
pub mod vertexarrayobject;
pub mod videotrack;
pub mod videotracklist;
pub mod virtualmethods;
pub mod vttcue;
pub mod vttregion;
pub mod webgl2renderingcontext;
pub mod webgl_extensions;
pub mod webgl_validations;
pub mod webglactiveinfo;
pub mod webglbuffer;
pub mod webglcontextevent;
pub mod webglframebuffer;
pub mod webglobject;
pub mod webglprogram;
pub mod webglquery;
pub mod webglrenderbuffer;
pub mod webglrenderingcontext;
pub mod webglsampler;
pub mod webglshader;
pub mod webglshaderprecisionformat;
pub mod webglsync;
pub mod webgltexture;
pub mod webgltransformfeedback;
pub mod webgluniformlocation;
pub mod webglvertexarrayobject;
pub mod webglvertexarrayobjectoes;
pub mod websocket;
pub mod wheelevent;
pub mod window;
pub mod windowproxy;
pub mod worker;
pub mod workerglobalscope;
pub mod workerlocation;
pub mod workernavigator;
pub mod worklet;
pub mod workletglobalscope;
pub mod xmldocument;
pub mod xmlhttprequest;
pub mod xmlhttprequesteventtarget;
pub mod xmlhttprequestupload;
pub mod xmlserializer;
pub mod xrcompositionlayer;
pub mod xrcubelayer;
pub mod xrcylinderlayer;
pub mod xrequirectlayer;
pub mod xrframe;
pub mod xrhand;
pub mod xrhittestresult;
pub mod xrhittestsource;
pub mod xrinputsource;
pub mod xrinputsourcearray;
pub mod xrinputsourceevent;
pub mod xrinputsourceschangeevent;
pub mod xrjointpose;
pub mod xrjointspace;
pub mod xrlayer;
pub mod xrlayerevent;
pub mod xrmediabinding;
pub mod xrpose;
pub mod xrprojectionlayer;
pub mod xrquadlayer;
pub mod xrray;
pub mod xrreferencespace;
pub mod xrrenderstate;
pub mod xrrigidtransform;
pub mod xrsession;
pub mod xrsessionevent;
pub mod xrspace;
pub mod xrsubimage;
pub mod xrsystem;
pub mod xrtest;
pub mod xrview;
pub mod xrviewerpose;
pub mod xrviewport;
pub mod xrwebglbinding;
pub mod xrwebgllayer;
pub mod xrwebglsubimage;
pub use self::webgl_extensions::ext::*;
