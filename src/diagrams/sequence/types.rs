// Port of src/diagrams/sequence/types.ts.

#![allow(non_upper_case_globals)]

/// The message kinds, numbered as in Mermaid.
pub mod linetype {
    pub const SOLID: u32 = 0;
    pub const DOTTED: u32 = 1;
    pub const NOTE: u32 = 2;
    pub const SOLID_CROSS: u32 = 3;
    pub const DOTTED_CROSS: u32 = 4;
    pub const SOLID_OPEN: u32 = 5;
    pub const DOTTED_OPEN: u32 = 6;
    pub const LOOP_START: u32 = 10;
    pub const LOOP_END: u32 = 11;
    pub const ALT_START: u32 = 12;
    pub const ALT_ELSE: u32 = 13;
    pub const ALT_END: u32 = 14;
    pub const OPT_START: u32 = 15;
    pub const OPT_END: u32 = 16;
    pub const ACTIVE_START: u32 = 17;
    pub const ACTIVE_END: u32 = 18;
    pub const PAR_START: u32 = 19;
    pub const PAR_AND: u32 = 20;
    pub const PAR_END: u32 = 21;
    pub const RECT_START: u32 = 22;
    pub const RECT_END: u32 = 23;
    pub const SOLID_POINT: u32 = 24;
    pub const DOTTED_POINT: u32 = 25;
    pub const AUTONUMBER: u32 = 26;
    pub const CRITICAL_START: u32 = 27;
    pub const CRITICAL_OPTION: u32 = 28;
    pub const CRITICAL_END: u32 = 29;
    pub const BREAK_START: u32 = 30;
    pub const BREAK_END: u32 = 31;
    pub const PAR_OVER_START: u32 = 32;
    pub const BIDIRECTIONAL_SOLID: u32 = 33;
    pub const BIDIRECTIONAL_DOTTED: u32 = 34;
    pub const SOLID_TOP: u32 = 41;
    pub const SOLID_BOTTOM: u32 = 42;
    pub const STICK_TOP: u32 = 43;
    pub const STICK_BOTTOM: u32 = 44;
    pub const SOLID_ARROW_TOP_REVERSE: u32 = 45;
    pub const SOLID_ARROW_BOTTOM_REVERSE: u32 = 46;
    pub const STICK_ARROW_TOP_REVERSE: u32 = 47;
    pub const STICK_ARROW_BOTTOM_REVERSE: u32 = 48;
    pub const SOLID_TOP_DOTTED: u32 = 51;
    pub const SOLID_BOTTOM_DOTTED: u32 = 52;
    pub const STICK_TOP_DOTTED: u32 = 53;
    pub const STICK_BOTTOM_DOTTED: u32 = 54;
    pub const SOLID_ARROW_TOP_REVERSE_DOTTED: u32 = 55;
    pub const SOLID_ARROW_BOTTOM_REVERSE_DOTTED: u32 = 56;
    pub const STICK_ARROW_TOP_REVERSE_DOTTED: u32 = 57;
    pub const STICK_ARROW_BOTTOM_REVERSE_DOTTED: u32 = 58;
    pub const CENTRAL_CONNECTION: u32 = 59;
    pub const CENTRAL_CONNECTION_REVERSE: u32 = 60;
    pub const CENTRAL_CONNECTION_DUAL: u32 = 61;
}

pub mod placement {
    pub const LEFTOF: u32 = 0;
    pub const RIGHTOF: u32 = 1;
    pub const OVER: u32 = 2;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeqText {
    pub text: String,
    pub wrap: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct SeqBoxData {
    pub text: Option<String>,
    pub color: String,
    pub wrap: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct SeqBox {
    pub name: Option<String>,
    pub wrap: bool,
    pub fill: String,
    pub actor_keys: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SeqActor {
    /// Index into the db's boxes.
    pub box_: Option<usize>,
    pub name: String,
    pub description: String,
    pub wrap: bool,
    pub prev_actor: Option<String>,
    pub next_actor: Option<String>,
    /// The `class` of the actor's properties, the only one drawn.
    pub class: Option<crate::util::yaml::Value>,
    pub details: Option<String>,
    pub type_: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeqNumbering {
    pub start: Option<f64>,
    pub step: Option<f64>,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MessageText {
    Text(String),
    Numbering(SeqNumbering),
}

#[derive(Clone, Debug)]
pub struct SeqMessage {
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub message: MessageText,
    pub wrap: bool,
    pub type_: Option<u32>,
    pub activate: bool,
    pub placement: Option<u32>,
    pub central_connection: u32,
}

/// One entry of the statement list the parser hands to `apply`, as Mermaid's grammar builds it.
#[derive(Clone, Debug, Default)]
pub struct SeqStatement {
    pub type_: String,
    pub actor: Option<String>,
    /// For a note over two participants.
    pub actors: Option<[String; 2]>,
    pub draw: Option<String>,
    pub description: Option<SeqText>,
    pub config: Option<String>,
    pub signal_type: Option<u32>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub msg: Option<SeqText>,
    pub activate: bool,
    pub central_connection: Option<u32>,
    pub placement: Option<u32>,
    /// The text of a block, divider or note, whichever key the grammar names it by.
    pub text: Option<SeqText>,
    pub box_data: Option<SeqBoxData>,
    pub sequence_index: Option<f64>,
    pub sequence_index_step: Option<f64>,
    pub sequence_visible: bool,
}

#[derive(Clone, Debug)]
pub enum SeqItem {
    Stmt(SeqStatement),
    Text(String),
    List(Vec<SeqItem>),
}
