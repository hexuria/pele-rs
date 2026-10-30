// Port of src/diagrams/class/tokens.ts.

#![allow(non_upper_case_globals)]

pub const END: u32 = 0;
pub const ERROR: u32 = 1;
pub const CLASS_DIAGRAM: u32 = 2;
pub const NEWLINE: u32 = 3;
pub const EOF: u32 = 4;
pub const SQS: u32 = 5;
pub const STR: u32 = 6;
pub const SQE: u32 = 7;
pub const DOT: u32 = 8;
pub const GENERICTYPE: u32 = 9;
pub const LABEL: u32 = 10;
pub const acc_title: u32 = 11;
pub const acc_title_value: u32 = 12;
pub const acc_descr: u32 = 13;
pub const acc_descr_value: u32 = 14;
pub const acc_descr_multiline_value: u32 = 15;
pub const STRUCT_START: u32 = 16;
pub const STRUCT_STOP: u32 = 17;
pub const NAMESPACE: u32 = 18;
pub const STYLE_SEPARATOR: u32 = 19;
pub const ANNOTATION_START: u32 = 20;
pub const ANNOTATION_END: u32 = 21;
pub const CLASS: u32 = 22;
pub const MEMBER: u32 = 23;
pub const NOTE_FOR: u32 = 24;
pub const NOTE: u32 = 25;
pub const CLASSDEF: u32 = 26;
pub const ALPHA: u32 = 27;
pub const COMMA: u32 = 28;
pub const direction_tb: u32 = 29;
pub const direction_bt: u32 = 30;
pub const direction_rl: u32 = 31;
pub const direction_lr: u32 = 32;
pub const AGGREGATION: u32 = 33;
pub const EXTENSION: u32 = 34;
pub const COMPOSITION: u32 = 35;
pub const DEPENDENCY: u32 = 36;
pub const LOLLIPOP: u32 = 37;
pub const LINE: u32 = 38;
pub const DOTTED_LINE: u32 = 39;
pub const CALLBACK: u32 = 40;
pub const LINK: u32 = 41;
pub const LINK_TARGET: u32 = 42;
pub const CLICK: u32 = 43;
pub const CALLBACK_NAME: u32 = 44;
pub const CALLBACK_ARGS: u32 = 45;
pub const HREF: u32 = 46;
pub const STYLE: u32 = 47;
pub const CSSCLASS: u32 = 48;
pub const COLON: u32 = 49;
pub const BRKT: u32 = 50;
pub const PCT: u32 = 51;
pub const MINUS: u32 = 52;
pub const UNICODE_TEXT: u32 = 53;
pub const BQUOTE_STR: u32 = 54;
pub const EDGE_STATE: u32 = 55;
pub const EOF_IN_STRUCT: u32 = 56;
pub const OPEN_IN_STRUCT: u32 = 57;
pub const PLUS: u32 = 58;
pub const EQUALS: u32 = 59;
pub const PUNCTUATION: u32 = 60;

pub const TOKEN_NAMES: [&str; 61] = [
    "$end",
    "INVALID",
    "CLASS_DIAGRAM",
    "NEWLINE",
    "EOF",
    "SQS",
    "STR",
    "SQE",
    "DOT",
    "GENERICTYPE",
    "LABEL",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "STRUCT_START",
    "STRUCT_STOP",
    "NAMESPACE",
    "STYLE_SEPARATOR",
    "ANNOTATION_START",
    "ANNOTATION_END",
    "CLASS",
    "MEMBER",
    "NOTE_FOR",
    "NOTE",
    "CLASSDEF",
    "ALPHA",
    "COMMA",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "AGGREGATION",
    "EXTENSION",
    "COMPOSITION",
    "DEPENDENCY",
    "LOLLIPOP",
    "LINE",
    "DOTTED_LINE",
    "CALLBACK",
    "LINK",
    "LINK_TARGET",
    "CLICK",
    "CALLBACK_NAME",
    "CALLBACK_ARGS",
    "HREF",
    "STYLE",
    "CSSCLASS",
    "COLON",
    "BRKT",
    "PCT",
    "MINUS",
    "UNICODE_TEXT",
    "BQUOTE_STR",
    "EDGE_STATE",
    "EOF_IN_STRUCT",
    "OPEN_IN_STRUCT",
    "PLUS",
    "EQUALS",
    "PUNCTUATION",
];
