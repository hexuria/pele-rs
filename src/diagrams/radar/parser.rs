// Port of src/diagrams/radar/parser.ts.

use crate::diagrams::common::tokens::*;
use crate::errors::PeleError;
use crate::js;
use std::rc::Rc;

const RADAR_COLON: i32 = 0;
const RADAR: i32 = 1;
const SHOW_LEGEND: i32 = 2;
const GRATICULE: i32 = 3;
const CURVE: i32 = 4;
const TICKS: i32 = 5;
const AXIS: i32 = 6;
const MAX: i32 = 7;
const MIN: i32 = 8;
const COLON: i32 = 9;
const COMMA: i32 = 10;
const OPEN: i32 = 11;
const CLOSE: i32 = 12;
const OPEN_BRACE: i32 = 13;
const CLOSE_BRACE: i32 = 14;
const GRATICULE_VALUE: i32 = 15;
const BOOLEAN: i32 = 16;
const ACC_DESCR: i32 = 17;
const ACC_TITLE: i32 = 18;
const TITLE: i32 = 19;
const NUMBER: i32 = 20;
const STRING: i32 = 21;
const ID: i32 = 22;
const NEWLINE: i32 = 23;

fn at(src: &[u16], i: usize) -> u16 {
    src.get(i).copied().unwrap_or(0xffff)
}

fn is_w(c: u16) -> bool {
    c < 128 && js::is_word(c as u8 as char)
}

fn digits(src: &[u16], from: usize) -> usize {
    let mut i = from;
    while (48..=57).contains(&at(src, i)) {
        i += 1;
    }
    i
}

// The NUMBER pattern without its backtracking:
// /(?:[0-9]+\.[0-9]+(?!\.))|(?:0|[1-9][0-9]*(?!\.))/y
fn number_scanner() -> Scanner {
    let mut run_from = 0usize;
    let mut run_end = 0usize;
    Box::new(move |src: &[u16], a: usize, _| {
        if a < run_from || a >= run_end {
            run_from = a;
            run_end = digits(src, a);
        }
        let whole = run_end;
        if whole == a {
            return None;
        }
        let dot = at(src, whole) == 46;
        if dot {
            let fraction = digits(src, whole + 1);
            let length = fraction - whole - 1;
            if length > 0 && at(src, fraction) != 46 {
                return Some(fraction);
            }
            if length > 1 {
                return Some(fraction - 1);
            }
        }
        if at(src, a) == 48 {
            return Some(a + 1);
        }
        if !dot {
            return Some(whole);
        }
        if whole - 1 > a {
            Some(whole - 1)
        } else {
            None
        }
    })
}

// `[\t ]*accDescr\s*{`
fn block_open(src: &[u16], p: usize) -> Option<usize> {
    let mut q = blanks(src, p);
    if js::from16(&src[q.min(src.len())..(q + 8).min(src.len())]) != "accDescr" {
        return None;
    }
    q += 8;
    while q < src.len() && js::is_space_u(src[q] as u32) {
        q += 1;
    }
    (at(src, q) == 123).then_some(q + 1)
}

// An `accDescr {` that never closes would read to the end of the text from every place.
fn acc_descr_scanner() -> Scanner {
    let plain = acc_descr();
    let Pattern::Re(re) = plain.pattern else { unreachable!() };
    let mut last_brace: Option<Option<usize>> = None;
    Box::new(move |src: &[u16], p: usize, _| {
        if let Some(end) = block_open(src, p) {
            let lb = *last_brace.get_or_insert_with(|| src.iter().rposition(|&c| c == 125));
            if lb.is_none_or(|b| end > b) {
                return None;
            }
        }
        re(src, p)
    })
}

// `[\w]([-\w]*\w)?`
fn id_end(src: &[u16], p: usize) -> Option<usize> {
    if !is_w(at(src, p)) {
        return None;
    }
    let mut end = p + 1;
    let mut q = p + 1;
    while q < src.len() && (is_w(src[q]) || src[q] == 45) {
        if is_w(src[q]) {
            end = q + 1;
        }
        q += 1;
    }
    Some(end)
}

// `[-\w]*\w` at p.
fn name_rest(src: &[u16], p: usize) -> bool {
    let mut q = p;
    while q < src.len() && (is_w(src[q]) || src[q] == 45) {
        if is_w(src[q]) {
            return true;
        }
        q += 1;
    }
    false
}

fn starts(src: &[u16], p: usize, word: &str) -> bool {
    word.encode_utf16().enumerate().all(|(i, u)| src.get(p + i) == Some(&u))
}

// Gives way to a longer name, except right after its option, as in `graticule polygonaxis`.
fn option_value(words: &'static [&'static str], option: i32) -> Rc<dyn Fn(&[u16], usize, i32) -> Option<usize>> {
    Rc::new(move |src, p, previous| {
        let w = words.iter().find(|w| starts(src, p, w))?;
        let end = p + w.len();
        if previous == option {
            return Some(end);
        }
        if name_rest(src, end) {
            None
        } else {
            Some(end)
        }
    })
}

pub fn radar_tokens() -> Vec<TokenType> {
    const ID_INDEX: usize = ID as usize;
    let word = |text: &'static str| {
        let mut t = TokenType::lit(text, text);
        t.longer = vec![ID_INDEX];
        t
    };
    let mark = |text: &'static str| TokenType::lit(text, text);
    let mut radar = keyword("radar-beta");
    radar.longer = vec![ID_INDEX];
    let mut graticule = TokenType::re("GRATICULE", None, |_, _| None);
    graticule.match_ = Some(option_value(&["circle", "polygon"], GRATICULE));
    let mut boolean = TokenType::re("BOOLEAN", None, |_, _| None);
    boolean.match_ = Some(option_value(&["true", "false"], SHOW_LEGEND));
    let mut descr = acc_descr();
    descr.scanner = Some(Rc::new(acc_descr_scanner));
    let mut number = TokenType::re("NUMBER", None, |_, _| None);
    number.scanner = Some(Rc::new(number_scanner));
    vec![
        mark("radar-beta:"),
        radar,
        word("showLegend"),
        word("graticule"),
        word("curve"),
        word("ticks"),
        word("axis"),
        word("max"),
        word("min"),
        mark(":"),
        mark(","),
        mark("["),
        mark("]"),
        mark("{"),
        mark("}"),
        graticule,
        boolean,
        descr,
        acc_title(),
        title_before(name_rest),
        number,
        string(),
        TokenType::re("ID", None, id_end),
        newline_token(),
        whitespace(),
        yaml(),
        directive(),
        single_line_comment(),
    ]
}

pub struct AxisAst {
    pub name: String,
    pub label: Option<String>,
}

pub struct CurveAst {
    pub name: String,
    pub label: Option<String>,
    // (axis reference text, value)
    pub entries: Vec<(Option<String>, f64)>,
}

pub enum OptionValue {
    Bool(bool),
    Num(f64),
    Str(String),
}

#[derive(Default)]
pub struct RadarAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub axes: Vec<AxisAst>,
    pub curves: Vec<CurveAst>,
    pub options: Vec<(String, OptionValue)>,
}

fn newlines(r: &mut Reader) {
    while r.kind() == NEWLINE {
        r.i += 1;
    }
}

fn end_of_line(r: &mut Reader) -> Result<(), PeleError> {
    if r.kind() == -1 {
        return Ok(());
    }
    if r.kind() != NEWLINE {
        return Err(r.fail("a line break or the end of input"));
    }
    newlines(r);
    Ok(())
}

fn label(r: &mut Reader) -> Result<Option<String>, PeleError> {
    if !r.accept(OPEN) {
        return Ok(None);
    }
    let text = string_value(&r.expect(STRING)?);
    r.expect(CLOSE)?;
    Ok(Some(text))
}

fn entry(r: &mut Reader, detailed: bool) -> Result<(Option<String>, f64), PeleError> {
    if !detailed {
        return Ok((None, js::to_number(&r.expect(NUMBER)?)));
    }
    let axis = r.expect(ID)?;
    r.accept(COLON);
    Ok((Some(axis), js::to_number(&r.expect(NUMBER)?)))
}

fn curve(r: &mut Reader) -> Result<CurveAst, PeleError> {
    let name = r.expect(ID)?;
    let label = label(r)?;
    let mut node = CurveAst { name, label, entries: Vec::new() };
    r.expect(OPEN_BRACE)?;
    newlines(r);
    if r.kind() != NUMBER && r.kind() != ID {
        return Err(r.fail("a number or an axis name"));
    }
    let detailed = r.kind() == ID;
    node.entries.push(entry(r, detailed)?);
    while r.accept(COMMA) {
        newlines(r);
        node.entries.push(entry(r, detailed)?);
    }
    newlines(r);
    r.expect(CLOSE_BRACE)?;
    Ok(node)
}

fn axis(r: &mut Reader) -> Result<AxisAst, PeleError> {
    let name = r.expect(ID)?;
    Ok(AxisAst { name, label: label(r)? })
}

fn option(r: &mut Reader) -> Result<(String, OptionValue), PeleError> {
    let kind = r.kind();
    let name = r.take();
    let value = if kind == SHOW_LEGEND {
        OptionValue::Bool(r.expect(BOOLEAN)? == "true")
    } else if kind == GRATICULE {
        OptionValue::Str(r.expect(GRATICULE_VALUE)?)
    } else {
        OptionValue::Num(js::to_number(&r.expect(NUMBER)?))
    };
    Ok((name, value))
}

fn is_option(r: &Reader) -> bool {
    matches!(r.kind(), SHOW_LEGEND | TICKS | MAX | MIN | GRATICULE)
}

pub fn parse_radar(src: &[u16]) -> Result<RadarAst, PeleError> {
    let types = radar_tokens();
    let tokens = tokenize(src, &types, "radar")?;
    let mut r = Reader::new(&tokens, &types, "radar");
    let mut ast = RadarAst::default();
    newlines(&mut r);
    if !r.accept(RADAR_COLON) {
        r.expect(RADAR)?;
        r.accept(COLON);
    }
    newlines(&mut r);
    while r.kind() != -1 {
        match r.kind() {
            NEWLINE => r.i += 1,
            ACC_DESCR => {
                ast.acc_descr = Some(acc_descr_value(&r.take()));
                end_of_line(&mut r)?;
            }
            ACC_TITLE => {
                ast.acc_title = Some(acc_title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            TITLE => {
                ast.title = Some(title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            AXIS => {
                r.i += 1;
                loop {
                    ast.axes.push(axis(&mut r)?);
                    if !r.accept(COMMA) {
                        break;
                    }
                }
            }
            CURVE => {
                r.i += 1;
                loop {
                    ast.curves.push(curve(&mut r)?);
                    if !r.accept(COMMA) {
                        break;
                    }
                }
            }
            _ => {
                if !is_option(&r) {
                    return Err(r.fail("an axis, a curve, an option, a title, or a line break"));
                }
                ast.options.push(option(&mut r)?);
                while r.accept(COMMA) {
                    if !is_option(&r) {
                        return Err(r.fail("an option"));
                    }
                    ast.options.push(option(&mut r)?);
                }
            }
        }
    }
    Ok(ast)
}
