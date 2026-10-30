// Port of src/diagrams/state/tokens.ts.

#![allow(non_upper_case_globals)]

pub const END: u32 = 0;
pub const ERROR: u32 = 1;
pub const NL: u32 = 2;
pub const SD: u32 = 3;
pub const DESCR: u32 = 4;
pub const ARROW: u32 = 5;
pub const HIDE_EMPTY: u32 = 6;
pub const scale: u32 = 7;
pub const WIDTH: u32 = 8;
pub const COMPOSIT_STATE: u32 = 9;
pub const STRUCT_START: u32 = 10;
pub const STRUCT_STOP: u32 = 11;
pub const STATE_DESCR: u32 = 12;
pub const AS: u32 = 13;
pub const ID: u32 = 14;
pub const FORK: u32 = 15;
pub const JOIN: u32 = 16;
pub const CHOICE: u32 = 17;
pub const CONCURRENT: u32 = 18;
pub const note: u32 = 19;
pub const NOTE_TEXT: u32 = 20;
pub const acc_title: u32 = 21;
pub const acc_title_value: u32 = 22;
pub const acc_descr: u32 = 23;
pub const acc_descr_value: u32 = 24;
pub const acc_descr_multiline_value: u32 = 25;
pub const CLICK: u32 = 26;
pub const STRING: u32 = 27;
pub const HREF: u32 = 28;
pub const classDef: u32 = 29;
pub const CLASSDEF_ID: u32 = 30;
pub const DEFAULT_CLASSDEF_ID: u32 = 31;
pub const CLASSDEF_STYLEOPTS: u32 = 32;
pub const DEFAULT: u32 = 33;
pub const style: u32 = 34;
pub const STYLE_IDS: u32 = 35;
pub const STYLEDEF_STYLEOPTS: u32 = 36;
pub const class_: u32 = 37;
pub const CLASSENTITY_IDS: u32 = 38;
pub const STYLECLASS: u32 = 39;
pub const direction_tb: u32 = 40;
pub const direction_bt: u32 = 41;
pub const direction_rl: u32 = 42;
pub const direction_lr: u32 = 43;
pub const EDGE_STATE: u32 = 44;
pub const STYLE_SEPARATOR: u32 = 45;
pub const left_of: u32 = 46;
pub const right_of: u32 = 47;
pub const INVALID: u32 = 48;

pub const TOKEN_NAMES: [&str; 49] = [
    "$end",
    "INVALID",
    "NL",
    "SD",
    "DESCR",
    "-->",
    "HIDE_EMPTY",
    "scale",
    "WIDTH",
    "COMPOSIT_STATE",
    "STRUCT_START",
    "STRUCT_STOP",
    "STATE_DESCR",
    "AS",
    "ID",
    "FORK",
    "JOIN",
    "CHOICE",
    "CONCURRENT",
    "note",
    "NOTE_TEXT",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "CLICK",
    "STRING",
    "HREF",
    "classDef",
    "CLASSDEF_ID",
    "DEFAULT_CLASSDEF_ID",
    "CLASSDEF_STYLEOPTS",
    "DEFAULT",
    "style",
    "STYLE_IDS",
    "STYLEDEF_STYLEOPTS",
    "class",
    "CLASSENTITY_IDS",
    "STYLECLASS",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "EDGE_STATE",
    "STYLE_SEPARATOR",
    "left_of",
    "right_of",
    "INVALID",
];
