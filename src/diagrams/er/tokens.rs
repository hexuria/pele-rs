// Port of src/diagrams/er/tokens.ts.

#![allow(non_upper_case_globals)]

pub const END: u32 = 0;
pub const ERROR: u32 = 1;
pub const ER_DIAGRAM: u32 = 2;
pub const EOF: u32 = 3;
pub const NEWLINE: u32 = 4;
pub const COLON: u32 = 5;
pub const STYLE_SEPARATOR: u32 = 6;
pub const BLOCK_START: u32 = 7;
pub const BLOCK_STOP: u32 = 8;
pub const SQS: u32 = 9;
pub const SQE: u32 = 10;
pub const acc_title: u32 = 11;
pub const acc_title_value: u32 = 12;
pub const acc_descr: u32 = 13;
pub const acc_descr_value: u32 = 14;
pub const acc_descr_multiline_value: u32 = 15;
pub const END_KW: u32 = 16;
pub const SUBGRAPH: u32 = 17;
pub const direction_tb: u32 = 18;
pub const direction_bt: u32 = 19;
pub const direction_rl: u32 = 20;
pub const direction_lr: u32 = 21;
pub const CLASSDEF: u32 = 22;
pub const UNICODE_TEXT: u32 = 23;
pub const STYLE_TEXT: u32 = 24;
pub const COMMA: u32 = 25;
pub const CLASS: u32 = 26;
pub const STYLE: u32 = 27;
pub const SEMI: u32 = 28;
pub const NUM: u32 = 29;
pub const BRKT: u32 = 30;
pub const ENTITY_NAME: u32 = 31;
pub const DECIMAL_NUM: u32 = 32;
pub const ENTITY_ONE: u32 = 33;
pub const ATTRIBUTE_WORD: u32 = 34;
pub const ATTRIBUTE_KEY: u32 = 35;
pub const COMMENT: u32 = 36;
pub const ZERO_OR_ONE: u32 = 37;
pub const ZERO_OR_MORE: u32 = 38;
pub const ONE_OR_MORE: u32 = 39;
pub const ONLY_ONE: u32 = 40;
pub const MD_PARENT: u32 = 41;
pub const NON_IDENTIFYING: u32 = 42;
pub const IDENTIFYING: u32 = 43;
pub const WORD: u32 = 44;
// A character no rule names; the grammar gives it meaning only as `?` and `,` inside a block.
pub const CHAR: u32 = 45;

pub const TOKEN_NAMES: [&str; 46] = [
    "$end",
    "INVALID",
    "ER_DIAGRAM",
    "EOF",
    "NEWLINE",
    "COLON",
    "STYLE_SEPARATOR",
    "BLOCK_START",
    "BLOCK_STOP",
    "SQS",
    "SQE",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "END",
    "SUBGRAPH",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "CLASSDEF",
    "UNICODE_TEXT",
    "STYLE_TEXT",
    "COMMA",
    "CLASS",
    "STYLE",
    "SEMI",
    "NUM",
    "BRKT",
    "ENTITY_NAME",
    "DECIMAL_NUM",
    "ENTITY_ONE",
    "ATTRIBUTE_WORD",
    "ATTRIBUTE_KEY",
    "COMMENT",
    "ZERO_OR_ONE",
    "ZERO_OR_MORE",
    "ONE_OR_MORE",
    "ONLY_ONE",
    "MD_PARENT",
    "NON_IDENTIFYING",
    "IDENTIFYING",
    "WORD",
    "CHAR",
];
