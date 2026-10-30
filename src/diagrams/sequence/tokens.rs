// Port of src/diagrams/sequence/tokens.ts.

#![allow(non_upper_case_globals)]

pub const END: u32 = 0;
pub const ERROR: u32 = 1;
pub const INVALID: u32 = 2;
pub const NEWLINE: u32 = 3;
pub const SD: u32 = 4;
pub const create: u32 = 5;
pub const box_: u32 = 6;
pub const restOfLine: u32 = 7;
pub const end: u32 = 8;
pub const autonumber: u32 = 9;
pub const NUM: u32 = 10;
pub const off: u32 = 11;
pub const activate: u32 = 12;
pub const deactivate: u32 = 13;
pub const title: u32 = 14;
pub const legacy_title: u32 = 15;
pub const acc_title: u32 = 16;
pub const acc_title_value: u32 = 17;
pub const acc_descr: u32 = 18;
pub const acc_descr_value: u32 = 19;
pub const acc_descr_multiline_value: u32 = 20;
pub const loop_: u32 = 21;
pub const rect: u32 = 22;
pub const opt: u32 = 23;
pub const alt: u32 = 24;
pub const par: u32 = 25;
pub const par_over: u32 = 26;
pub const critical: u32 = 27;
pub const break_: u32 = 28;
pub const option: u32 = 29;
pub const and: u32 = 30;
pub const else_: u32 = 31;
pub const participant: u32 = 32;
pub const AS: u32 = 33;
pub const participant_actor: u32 = 34;
pub const destroy: u32 = 35;
pub const note: u32 = 36;
pub const over: u32 = 37;
pub const links: u32 = 38;
pub const link: u32 = 39;
pub const properties: u32 = 40;
pub const details: u32 = 41;
pub const COMMA: u32 = 42;
pub const left_of: u32 = 43;
pub const right_of: u32 = 44;
pub const PLUS: u32 = 45;
pub const MINUS: u32 = 46;
pub const CENTRAL: u32 = 47;
pub const ACTOR: u32 = 48;
pub const CONFIG_START: u32 = 49;
pub const CONFIG_CONTENT: u32 = 50;
pub const CONFIG_END: u32 = 51;
pub const TXT: u32 = 52;
pub const SOLID_OPEN_ARROW: u32 = 53;
pub const DOTTED_OPEN_ARROW: u32 = 54;
pub const SOLID_ARROW: u32 = 55;
pub const BIDIRECTIONAL_SOLID_ARROW: u32 = 56;
pub const DOTTED_ARROW: u32 = 57;
pub const BIDIRECTIONAL_DOTTED_ARROW: u32 = 58;
pub const SOLID_CROSS: u32 = 59;
pub const DOTTED_CROSS: u32 = 60;
pub const SOLID_POINT: u32 = 61;
pub const DOTTED_POINT: u32 = 62;
pub const SOLID_ARROW_TOP: u32 = 63;
pub const SOLID_ARROW_BOTTOM: u32 = 64;
pub const STICK_ARROW_TOP: u32 = 65;
pub const STICK_ARROW_BOTTOM: u32 = 66;
pub const SOLID_ARROW_TOP_DOTTED: u32 = 67;
pub const SOLID_ARROW_BOTTOM_DOTTED: u32 = 68;
pub const STICK_ARROW_TOP_DOTTED: u32 = 69;
pub const STICK_ARROW_BOTTOM_DOTTED: u32 = 70;
pub const SOLID_ARROW_TOP_REVERSE: u32 = 71;
pub const SOLID_ARROW_BOTTOM_REVERSE: u32 = 72;
pub const STICK_ARROW_TOP_REVERSE: u32 = 73;
pub const STICK_ARROW_BOTTOM_REVERSE: u32 = 74;
pub const SOLID_ARROW_TOP_REVERSE_DOTTED: u32 = 75;
pub const SOLID_ARROW_BOTTOM_REVERSE_DOTTED: u32 = 76;
pub const STICK_ARROW_TOP_REVERSE_DOTTED: u32 = 77;
pub const STICK_ARROW_BOTTOM_REVERSE_DOTTED: u32 = 78;

// Names as Mermaid's grammar spells them, for error messages.
pub const TOKEN_NAMES: [&str; 79] = [
    "$end",
    "INVALID",
    "INVALID",
    "NEWLINE",
    "SD",
    "create",
    "box",
    "restOfLine",
    "end",
    "autonumber",
    "NUM",
    "off",
    "activate",
    "deactivate",
    "title",
    "legacy_title",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "loop",
    "rect",
    "opt",
    "alt",
    "par",
    "par_over",
    "critical",
    "break",
    "option",
    "and",
    "else",
    "participant",
    "AS",
    "participant_actor",
    "destroy",
    "note",
    "over",
    "links",
    "link",
    "properties",
    "details",
    ",",
    "left_of",
    "right_of",
    "+",
    "-",
    "()",
    "ACTOR",
    "CONFIG_START",
    "CONFIG_CONTENT",
    "CONFIG_END",
    "TXT",
    "SOLID_OPEN_ARROW",
    "DOTTED_OPEN_ARROW",
    "SOLID_ARROW",
    "BIDIRECTIONAL_SOLID_ARROW",
    "DOTTED_ARROW",
    "BIDIRECTIONAL_DOTTED_ARROW",
    "SOLID_CROSS",
    "DOTTED_CROSS",
    "SOLID_POINT",
    "DOTTED_POINT",
    "SOLID_ARROW_TOP",
    "SOLID_ARROW_BOTTOM",
    "STICK_ARROW_TOP",
    "STICK_ARROW_BOTTOM",
    "SOLID_ARROW_TOP_DOTTED",
    "SOLID_ARROW_BOTTOM_DOTTED",
    "STICK_ARROW_TOP_DOTTED",
    "STICK_ARROW_BOTTOM_DOTTED",
    "SOLID_ARROW_TOP_REVERSE",
    "SOLID_ARROW_BOTTOM_REVERSE",
    "STICK_ARROW_TOP_REVERSE",
    "STICK_ARROW_BOTTOM_REVERSE",
    "SOLID_ARROW_TOP_REVERSE_DOTTED",
    "SOLID_ARROW_BOTTOM_REVERSE_DOTTED",
    "STICK_ARROW_TOP_REVERSE_DOTTED",
    "STICK_ARROW_BOTTOM_REVERSE_DOTTED",
];
