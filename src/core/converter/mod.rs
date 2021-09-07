/*!
IR Converter module takes AST and produces intermediate representation.
All core template syntax conversion happens here. IR is later used for
optimizing transformation and code generation. As we decouple codegen
node from AST, Vue's transformation passes are broken down to two parts.
Convert module roughly corresponds to following transform in vue-next.

# IR Convert
* transformElement
* transformSlotOutlet
* transformTextCall
* vFor
* vIf
* vSlot

# Transform directive
* noopDirectiveTransform
* vModel
* vBind
* vOn
*/

pub use super::error::ErrorHandler;
pub use super::parser::{AstNode, AstRoot, Directive, Element};
use super::util::find_dir;
use rustc_hash::FxHashMap;

mod v_bind;
mod v_on;

pub trait ConvertInfo {
    type TextType;
    type IfType;
    type ForType;
    type VNodeType;
    type RenderSlotType;
    type VSlotType;
    type GenericJSType;
}

pub enum VSlotExpr {
    /// stable v-slots declared statically in the template
    StableSlotObject,
    /// v-slots dynamically declared v-slot template with v-if/v-for
    DynamicSlotCall,
}

pub enum IRNode<T: ConvertInfo> {
    /// interpolation or text node
    TextCall(T::TextType),
    /// v-if, else-if, else
    If(T::IfType),
    /// v-for
    For(T::ForType),
    /// plain element or component
    VNodeCall(T::VNodeType),
    /// <slot> slot outlet
    RenderSlotCall(T::RenderSlotType),
    /// v-slot on component or template
    VSlotExpression(T::VSlotType),
    /// generic JS expression
    GenericExpression(T::GenericJSType),
}

struct IfNodeIR {}
struct ForNodeIR {}
struct VNodeIR {}

pub type Prop<'a> = (JsExpression<'a>, JsExpression<'a>);
pub enum JsExpression<'a> {
    Lit(&'a str),
    Simple(&'a str),
    Compound(Vec<JsExpression<'a>>),
    Props(Vec<Prop<'a>>),
    Call(&'static str, Vec<JsExpression<'a>>),
}

pub enum BindingTypes {
    /// returned from data()
    Data,
    /// declared as a prop
    Props,
    /// a let binding (may or may not be a ref)
    SetupLet,
    ///a const binding that can never be a ref.
    ///these bindings don't need `unref()` calls when processed in inlined
    ///template expressions.
    SetupConst,
    /// a const binding that may be a ref.
    SetupMaybeRef,
    /// bindings that are guaranteed to be refs
    SetupRef,
    /// declared by other options, e.g. computed, inject
    Options,
}
pub struct ConvertOption {
    pub directive_converters: Vec<DirectiveConverter>,
    pub binding_metadata: FxHashMap<&'static str, BindingTypes>,
}

pub struct IRRoot<T: ConvertInfo> {
    pub body: Vec<IRNode<T>>,
}

/// Converts template ast node to intermediate representation.
/// the IR format can be platform specific.
/// e.g SSR Codegen and DOM Codegen can have different IR
pub trait Converter<'a>: Sized {
    type IR;
    fn convert_ir(&self, ast: AstRoot<'a>) -> Self::IR;
}

/// Default implementation  sketch can be used in DOM/SSR.
/// Other platform might invent and use their own IR.
pub trait BuiltinConverter<'a, T>
where
    T: ConvertInfo,
    Self: Converter<'a, IR = IRRoot<T>>,
{
    fn convert_ir(&self, ast: AstRoot<'a>) -> Self::IR {
        let body = ast
            .children
            .into_iter()
            .map(|n| self.dispatch_ast(n))
            .collect();
        IRRoot { body }
    }
    fn dispatch_ast(&self, n: AstNode<'a>) -> IRNode<T> {
        match n {
            AstNode::Text(..) => self.convert_text(),
            AstNode::Comment(..) => self.convert_comment(),
            AstNode::Interpolation(..) => self.convert_interpolation(),
            // all element like node needs structural pre conversion
            AstNode::Plain(e) => self.pre_convert_structural_dir(e),
            AstNode::Component(e) => self.pre_convert_structural_dir(e),
            AstNode::Template(e) => self.pre_convert_structural_dir(e),
            // <slot> requires special v-if/v-for handling
            AstNode::SlotOutlet(..) => self.convert_slot_outlet(),
        }
    }
    // pre convert v-if or v-for like structural dir
    fn pre_convert_structural_dir(&self, mut e: Element<'a>) -> IRNode<T> {
        if let Some(dir) = find_dir(&mut e, ["if", "else-if", "else", "for"]) {
            let b = dir.take();
            let n = self.pre_convert_structural_dir(e);
            if b.name == "for" {
                self.convert_for(n)
            } else {
                self.convert_if(n)
            }
        } else {
            self.convert_element(e)
        }
    }
    // core template syntax conversion
    fn convert_directive(&self) -> IRNode<T>;
    fn convert_if(&self, n: IRNode<T>) -> IRNode<T>;
    fn convert_for(&self, n: IRNode<T>) -> IRNode<T>;
    fn convert_slot_outlet(&self) -> IRNode<T>;
    fn convert_element(&self, e: Element<'a>) -> IRNode<T>;
    fn convert_text(&self) -> IRNode<T>;
    fn convert_interpolation(&self) -> IRNode<T>;
    fn convert_template(&self, e: Element<'a>) -> IRNode<T>;
    fn convert_comment(&self) -> IRNode<T>;
}

/// Directive's prop argument passed to VNodeCall after conversion.
/// Use Dropped if the directive is dropped implicitly without codegen.
/// NB: this is not 100% translation from TS. `value` accepts both Props and Object.
// This design decouples v-bind/on from transform_element.
pub enum DirectiveConvertResult<'a> {
    Converted {
        value: JsExpression<'a>,
        need_runtime: bool,
    },
    Dropped,
}

/// Returns the conversion of a directive. Value could be props or object.
// NB: we pass &dyn ErrorHandler to monomorphize the dir converter to pay
// the minimal cost of dynamism only when error occurs. otherwise we will
// incur the overhead of dyn DirectiveConvert in the ConvertOption.
pub type DirConvertFn =
    for<'a> fn(Directive<'a>, &Element<'a>, &dyn ErrorHandler) -> DirectiveConvertResult<'a>;
pub type DirectiveConverter = (&'static str, DirConvertFn);
pub fn no_op_directive_convert<'a>(
    _: Directive<'a>,
    _: &Element<'a>,
    _: &dyn ErrorHandler,
) -> DirectiveConvertResult<'a> {
    DirectiveConvertResult::Dropped
}
