// Port of src/diagrams/sequence/parser.ts.

use super::db::SeqDb;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{linetype as lt, placement, SeqItem, SeqStatement};
use crate::errors::{syntax_error, PeleError};
use crate::js;

fn signal_of(ty: u32) -> Option<u32> {
    Some(match ty {
        t::SOLID_OPEN_ARROW => lt::SOLID_OPEN,
        t::DOTTED_OPEN_ARROW => lt::DOTTED_OPEN,
        t::SOLID_ARROW => lt::SOLID,
        t::DOTTED_ARROW => lt::DOTTED,
        t::BIDIRECTIONAL_SOLID_ARROW => lt::BIDIRECTIONAL_SOLID,
        t::BIDIRECTIONAL_DOTTED_ARROW => lt::BIDIRECTIONAL_DOTTED,
        t::SOLID_CROSS => lt::SOLID_CROSS,
        t::DOTTED_CROSS => lt::DOTTED_CROSS,
        t::SOLID_POINT => lt::SOLID_POINT,
        t::DOTTED_POINT => lt::DOTTED_POINT,
        t::SOLID_ARROW_TOP => lt::SOLID_TOP,
        t::SOLID_ARROW_BOTTOM => lt::SOLID_BOTTOM,
        t::STICK_ARROW_TOP => lt::STICK_TOP,
        t::STICK_ARROW_BOTTOM => lt::STICK_BOTTOM,
        t::SOLID_ARROW_TOP_DOTTED => lt::SOLID_TOP_DOTTED,
        t::SOLID_ARROW_BOTTOM_DOTTED => lt::SOLID_BOTTOM_DOTTED,
        t::STICK_ARROW_TOP_DOTTED => lt::STICK_TOP_DOTTED,
        t::STICK_ARROW_BOTTOM_DOTTED => lt::STICK_BOTTOM_DOTTED,
        t::SOLID_ARROW_TOP_REVERSE => lt::SOLID_ARROW_TOP_REVERSE,
        t::SOLID_ARROW_BOTTOM_REVERSE => lt::SOLID_ARROW_BOTTOM_REVERSE,
        t::STICK_ARROW_TOP_REVERSE => lt::STICK_ARROW_TOP_REVERSE,
        t::STICK_ARROW_BOTTOM_REVERSE => lt::STICK_ARROW_BOTTOM_REVERSE,
        t::SOLID_ARROW_TOP_REVERSE_DOTTED => lt::SOLID_ARROW_TOP_REVERSE_DOTTED,
        t::SOLID_ARROW_BOTTOM_REVERSE_DOTTED => lt::SOLID_ARROW_BOTTOM_REVERSE_DOTTED,
        t::STICK_ARROW_TOP_REVERSE_DOTTED => lt::STICK_ARROW_TOP_REVERSE_DOTTED,
        t::STICK_ARROW_BOTTOM_REVERSE_DOTTED => lt::STICK_ARROW_BOTTOM_REVERSE_DOTTED,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum K {
    Top,
    Box,
    Loop,
    Rect,
    Opt,
    Break,
    Alt,
    Par,
    Critical,
}

fn block_of(ty: u32) -> Option<K> {
    Some(match ty {
        t::loop_ => K::Loop,
        t::rect => K::Rect,
        t::opt => K::Opt,
        t::break_ => K::Break,
        t::alt => K::Alt,
        t::par | t::par_over => K::Par,
        t::critical => K::Critical,
        t::box_ => K::Box,
        _ => return None,
    })
}

enum Section {
    List(Vec<SeqItem>),
    Text(String),
}

struct Frame {
    kind: K,
    text: String,
    start_type: u32,
    // The current section's statements. Slot 0 of a block's first section is kept for its start entry.
    list: Vec<SeqItem>,
    // Finished sections, each followed by the divider text that ended it.
    sections: Option<Vec<Section>>,
}

const SIGNAL_TOKENS: &str = "'SOLID_OPEN_ARROW', 'DOTTED_OPEN_ARROW', 'SOLID_ARROW', 'DOTTED_ARROW', 'SOLID_CROSS', 'DOTTED_CROSS', 'SOLID_POINT', 'DOTTED_POINT'";

struct P<'a> {
    src: &'a [u16],
    types: Vec<u32>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
}

type R<T> = Result<T, PeleError>;

fn stmt(type_: &str) -> SeqStatement {
    SeqStatement { type_: type_.to_string(), ..Default::default() }
}

impl P<'_> {
    fn ty(&self) -> u32 {
        self.types.get(self.i).copied().unwrap_or(t::END)
    }

    fn ty_at(&self, i: usize) -> u32 {
        self.types.get(i).copied().unwrap_or(u32::MAX)
    }

    fn take(&mut self) -> String {
        let s = self.texts[self.i].clone();
        self.i += 1;
        s
    }

    fn fail(&self, expected: &str) -> PeleError {
        let ty = self.ty();
        if ty == t::ERROR {
            return syntax_error("sequence", self.src, self.starts[self.i], "", true);
        }
        syntax_error("sequence", self.src, self.starts[self.i], &format!("Expecting {}, got '{}'", expected, TOKEN_NAMES[ty as usize]), false)
    }

    fn expect(&mut self, ty: u32) -> R<String> {
        if self.ty() != ty {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize])));
        }
        Ok(self.take())
    }

    fn actor(&mut self) -> R<SeqStatement> {
        let ty = self.ty();
        if ty != t::ACTOR && ty != t::link {
            return Err(self.fail("'ACTOR'"));
        }
        let mut s = stmt("addParticipant");
        s.actor = Some(self.take());
        Ok(s)
    }

    fn text2(&mut self, db: &SeqDb) -> R<super::types::SeqText> {
        let raw = self.expect(t::TXT)?;
        let trimmed = js::u16s(js::trim(&raw));
        Ok(db.parse_message(&js::from16(&trimmed[1.min(trimmed.len())..])))
    }

    fn participant(&mut self, db: &SeqDb) -> R<SeqStatement> {
        let ty = self.ty();
        self.i += 1;
        if ty == t::destroy {
            let mut node = self.actor()?;
            self.expect(t::NEWLINE)?;
            node.type_ = "destroyParticipant".into();
            return Ok(node);
        }
        let mut node = self.actor()?;
        if self.ty() == t::CONFIG_START && self.ty_at(self.i - 1) == t::ACTOR {
            self.i += 1;
            let content = self.expect(t::CONFIG_CONTENT)?;
            self.expect(t::CONFIG_END)?;
            node.config = Some(js::trim(&content).to_string());
        }
        node.draw = Some(if ty == t::participant { "participant" } else { "actor" }.into());
        if self.ty() == t::AS {
            self.i += 1;
            let alias = self.expect(t::restOfLine)?;
            self.expect(t::NEWLINE)?;
            node.description = Some(db.parse_message(&alias));
        } else if self.ty() == t::NEWLINE {
            self.i += 1;
        } else {
            return Err(self.fail("'AS', 'NEWLINE'"));
        }
        Ok(node)
    }

    fn signal(&mut self, from: SeqStatement, db: &SeqDb) -> R<SeqItem> {
        let mut central_from = false;
        if self.ty() == t::CENTRAL {
            central_from = true;
            self.i += 1;
        }
        let Some(signal_type) = signal_of(self.ty()) else { return Err(self.fail(SIGNAL_TOKENS)) };
        self.i += 1;
        let mark = self.ty();
        if mark == t::CENTRAL || (!central_from && (mark == t::PLUS || mark == t::MINUS)) {
            self.i += 1;
        }
        let to = self.actor()?;
        let msg = self.text2(db)?;
        let mut message = stmt("addMessage");
        message.from = from.actor.clone();
        message.to = to.actor.clone();
        message.signal_type = Some(signal_type);
        message.msg = Some(msg);
        let to_actor = to.actor.clone();
        let from_actor = from.actor.clone();
        let mut extra: Vec<SeqItem> = Vec::new();
        let with = |type_: &str, signal: u32, actor: Option<String>| {
            let mut s = stmt(type_);
            s.signal_type = Some(signal);
            s.actor = actor;
            SeqItem::Stmt(s)
        };
        if central_from {
            let dual = mark == t::CENTRAL;
            message.activate = dual;
            message.central_connection = Some(if dual { lt::CENTRAL_CONNECTION_DUAL } else { lt::CENTRAL_CONNECTION_REVERSE });
            if dual {
                extra.push(with("centralConnection", lt::CENTRAL_CONNECTION, to_actor.clone()));
            }
            extra.push(with("centralConnectionReverse", lt::CENTRAL_CONNECTION_REVERSE, from_actor));
        } else if mark == t::PLUS {
            message.activate = true;
            extra.push(with("activeStart", lt::ACTIVE_START, to_actor));
        } else if mark == t::MINUS {
            extra.push(with("activeEnd", lt::ACTIVE_END, from_actor));
        } else if mark == t::CENTRAL {
            message.activate = true;
            message.central_connection = Some(lt::CENTRAL_CONNECTION);
            extra.push(with("centralConnection", lt::CENTRAL_CONNECTION, to_actor));
        }
        let mut out = vec![SeqItem::Stmt(from), SeqItem::Stmt(to), SeqItem::Stmt(message)];
        out.extend(extra);
        Ok(SeqItem::List(out))
    }

    fn menu(&mut self, type_: &str, db: &SeqDb) -> R<SeqItem> {
        self.i += 1;
        let who = self.actor()?;
        let mut s = stmt(type_);
        s.actor = who.actor.clone();
        s.text = Some(self.text2(db)?);
        Ok(SeqItem::List(vec![SeqItem::Stmt(who), SeqItem::Stmt(s)]))
    }

    fn note(&mut self, db: &SeqDb) -> R<SeqItem> {
        self.i += 1;
        let ty = self.ty();
        if ty == t::over {
            self.i += 1;
            let first = self.actor()?;
            let a = first.actor.clone().unwrap_or_default();
            let mut ids = [a.clone(), a];
            let who = if self.ty() == t::COMMA {
                self.i += 1;
                let second = self.actor()?;
                ids[1] = second.actor.clone().unwrap_or_default();
                SeqItem::List(vec![SeqItem::Stmt(first), SeqItem::Stmt(second)])
            } else {
                SeqItem::Stmt(first)
            };
            let mut s = stmt("addNote");
            s.placement = Some(placement::OVER);
            s.actors = Some(ids);
            s.text = Some(self.text2(db)?);
            return Ok(SeqItem::List(vec![who, SeqItem::Stmt(s)]));
        }
        if ty != t::left_of && ty != t::right_of {
            return Err(self.fail("'over', 'left_of', 'right_of'"));
        }
        self.i += 1;
        let who = self.actor()?;
        let mut s = stmt("addNote");
        s.placement = Some(if ty == t::left_of { placement::LEFTOF } else { placement::RIGHTOF });
        s.actor = who.actor.clone();
        s.text = Some(self.text2(db)?);
        Ok(SeqItem::List(vec![SeqItem::Stmt(who), SeqItem::Stmt(s)]))
    }

    fn autonumber(&mut self) -> R<SeqStatement> {
        self.i += 1;
        let mut node = stmt("sequenceIndex");
        node.sequence_visible = true;
        node.signal_type = Some(lt::AUTONUMBER);
        if self.ty() == t::NUM {
            node.sequence_index = Some(js::to_number(&self.take()));
            node.sequence_index_step = Some(if self.ty() == t::NUM { js::to_number(&self.take()) } else { 1.0 });
        } else if self.ty() == t::off {
            self.i += 1;
            node.sequence_visible = false;
        }
        if self.ty() != t::NEWLINE {
            return Err(self.fail("'NEWLINE', 'NUM', 'off'"));
        }
        self.i += 1;
        Ok(node)
    }
}

// Folds the sections of an alt, par, or critical block from the last one back, as the grammar's
// right recursion does, so the texts reach the model builder in the same order.
fn fold(frame: &mut Frame, divider: &str, signal_type: u32, db: &SeqDb) -> Vec<SeqItem> {
    let mut list = std::mem::take(&mut frame.list);
    if let Some(sections) = frame.sections.take() {
        let mut sections: Vec<Section> = sections;
        let mut k = sections.len() as isize - 2;
        while k >= 0 {
            let text = match &sections[k as usize + 1] {
                Section::Text(s) => s.clone(),
                _ => String::new(),
            };
            let mut head = stmt(divider);
            head.signal_type = Some(signal_type);
            head.text = Some(db.parse_message(&text));
            let section = match std::mem::replace(&mut sections[k as usize], Section::Text(String::new())) {
                Section::List(l) => l,
                _ => Vec::new(),
            };
            let mut next = section;
            next.push(SeqItem::Stmt(head));
            next.push(SeqItem::List(list));
            list = next;
            k -= 2;
        }
    }
    list
}

fn close(mut frame: Frame, db: &SeqDb) -> Vec<SeqItem> {
    let text = frame.text.clone();
    let with = |type_: &str, signal: Option<u32>, text: Option<super::types::SeqText>| {
        let mut s = stmt(type_);
        s.signal_type = signal;
        s.text = text;
        SeqItem::Stmt(s)
    };
    let msg = |t: &str| Some(db.parse_message(t));
    let mut list;
    match frame.kind {
        K::Box => {
            list = std::mem::take(&mut frame.list);
            let mut s = stmt("boxStart");
            s.box_data = Some(db.parse_box_data(&text));
            list[0] = SeqItem::Stmt(s);
            list.push(with("boxEnd", None, None));
        }
        K::Loop => {
            list = std::mem::take(&mut frame.list);
            list[0] = with("loopStart", Some(lt::LOOP_START), msg(&text));
            list.push(with("loopEnd", Some(lt::LOOP_END), None));
        }
        K::Rect => {
            list = std::mem::take(&mut frame.list);
            list[0] = with("rectStart", Some(lt::RECT_START), msg(&text));
            list.push(with("rectEnd", Some(lt::RECT_END), msg(&text)));
        }
        K::Opt => {
            list = std::mem::take(&mut frame.list);
            list[0] = with("optStart", Some(lt::OPT_START), msg(&text));
            list.push(with("optEnd", Some(lt::OPT_END), msg(&text)));
        }
        K::Break => {
            list = std::mem::take(&mut frame.list);
            list[0] = with("breakStart", Some(lt::BREAK_START), msg(&text));
            list.push(with("breakEnd", Some(lt::BREAK_END), msg(&text)));
        }
        K::Alt => {
            list = fold(&mut frame, "else", lt::ALT_ELSE, db);
            list[0] = with("altStart", Some(lt::ALT_START), msg(&text));
            list.push(with("altEnd", Some(lt::ALT_END), None));
        }
        K::Par => {
            list = fold(&mut frame, "and", lt::PAR_AND, db);
            list[0] = with("parStart", Some(frame.start_type), msg(&text));
            list.push(with("parEnd", Some(lt::PAR_END), None));
        }
        _ => {
            list = fold(&mut frame, "option", lt::CRITICAL_OPTION, db);
            list[0] = with("criticalStart", Some(lt::CRITICAL_START), msg(&text));
            list.push(with("criticalEnd", Some(lt::CRITICAL_END), None));
        }
    }
    list
}

pub fn parse_sequence(src: &[u16], db: &mut SeqDb) -> R<()> {
    let tokens = tokenize(src);
    let mut p = P { src, types: tokens.types, texts: tokens.texts, starts: tokens.starts, i: 0 };
    while p.ty() == t::NEWLINE {
        p.i += 1;
    }
    p.expect(t::SD)?;

    let mut stack: Vec<Frame> = Vec::new();
    let mut frame = Frame { kind: K::Top, text: String::new(), start_type: 0, list: Vec::new(), sections: None };

    loop {
        let ty = p.ty();
        if frame.kind == K::Box {
            if ty == t::participant || ty == t::participant_actor || ty == t::destroy {
                let s = p.participant(db)?;
                frame.list.push(SeqItem::Stmt(s));
            } else if ty == t::NEWLINE {
                frame.list.push(SeqItem::List(Vec::new()));
                p.i += 1;
            } else if ty == t::end {
                p.i += 1;
                let done = close(frame, db);
                frame = stack.pop().unwrap();
                frame.list.push(SeqItem::List(done));
            } else {
                return Err(p.fail("'end', 'NEWLINE', 'participant', 'participant_actor', 'destroy'"));
            }
            continue;
        }
        match ty {
            t::NEWLINE | t::INVALID => {
                frame.list.push(SeqItem::List(Vec::new()));
                p.i += 1;
            }
            t::participant | t::participant_actor | t::destroy => {
                let s = p.participant(db)?;
                frame.list.push(SeqItem::Stmt(s));
            }
            t::create => {
                p.i += 1;
                let next = p.ty();
                if next != t::participant && next != t::participant_actor && next != t::destroy {
                    return Err(p.fail("'participant', 'participant_actor', 'destroy'"));
                }
                let mut node = p.participant(db)?;
                node.type_ = "createParticipant".into();
                frame.list.push(SeqItem::Stmt(node));
            }
            t::ACTOR => {
                let a = p.actor()?;
                let s = p.signal(a, db)?;
                frame.list.push(s);
                p.expect(t::NEWLINE)?;
            }
            t::link => {
                let next = p.ty_at(p.i + 1);
                let item = if next == t::ACTOR || next == t::link {
                    p.menu("addALink", db)?
                } else {
                    let a = p.actor()?;
                    p.signal(a, db)?
                };
                frame.list.push(item);
                p.expect(t::NEWLINE)?;
            }
            t::links | t::properties | t::details => {
                let kind = match ty {
                    t::links => "addLinks",
                    t::properties => "addProperties",
                    _ => "addDetails",
                };
                let item = p.menu(kind, db)?;
                frame.list.push(item);
                p.expect(t::NEWLINE)?;
            }
            t::note => {
                let item = p.note(db)?;
                frame.list.push(item);
                p.expect(t::NEWLINE)?;
            }
            t::autonumber => {
                let s = p.autonumber()?;
                frame.list.push(SeqItem::Stmt(s));
            }
            t::activate | t::deactivate => {
                p.i += 1;
                let who = p.actor()?;
                p.expect(t::NEWLINE)?;
                let mut s = stmt(if ty == t::activate { "activeStart" } else { "activeEnd" });
                s.signal_type = Some(if ty == t::activate { lt::ACTIVE_START } else { lt::ACTIVE_END });
                s.actor = who.actor;
                frame.list.push(SeqItem::Stmt(s));
            }
            t::title | t::legacy_title => {
                let raw = js::u16s(&p.take());
                let skip = if ty == t::title { 6 } else { 7 };
                let title = js::from16(&raw[skip.min(raw.len())..]);
                db.set_diagram_title(&title);
                frame.list.push(SeqItem::Text(title));
            }
            t::acc_title => {
                p.i += 1;
                let value = js::trim(&p.expect(t::acc_title_value)?).to_string();
                db.set_acc_title(&value);
                frame.list.push(SeqItem::Text(value));
            }
            t::acc_descr => {
                p.i += 1;
                let value = js::trim(&p.expect(t::acc_descr_value)?).to_string();
                db.set_acc_description(&value);
                frame.list.push(SeqItem::Text(value));
            }
            t::acc_descr_multiline_value => {
                let value = js::trim(&p.take()).to_string();
                db.set_acc_description(&value);
                frame.list.push(SeqItem::Text(value));
            }
            t::loop_ | t::rect | t::opt | t::break_ | t::alt | t::par | t::par_over | t::critical | t::box_ => {
                p.i += 1;
                let text = p.expect(t::restOfLine)?;
                let outer = std::mem::replace(
                    &mut frame,
                    Frame { kind: block_of(ty).unwrap(), text, start_type: if ty == t::par_over { lt::PAR_OVER_START } else { lt::PAR_START }, list: vec![SeqItem::List(Vec::new())], sections: None },
                );
                stack.push(outer);
            }
            t::else_ | t::and | t::option => {
                let want = if ty == t::else_ {
                    K::Alt
                } else if ty == t::and {
                    K::Par
                } else {
                    K::Critical
                };
                if frame.kind != want {
                    return Err(p.fail("'end'"));
                }
                p.i += 1;
                let text = p.expect(t::restOfLine)?;
                let list = std::mem::take(&mut frame.list);
                let sections = frame.sections.get_or_insert_with(Vec::new);
                sections.push(Section::List(list));
                sections.push(Section::Text(text));
            }
            t::end => {
                if frame.kind == K::Top {
                    return Err(p.fail("'NEWLINE', 'EOF'"));
                }
                p.i += 1;
                let done = close(frame, db);
                frame = stack.pop().unwrap();
                frame.list.push(SeqItem::List(done));
            }
            t::END => {
                if frame.kind != K::Top {
                    return Err(p.fail("'end'"));
                }
                let list = std::mem::take(&mut frame.list);
                return db.apply(&SeqItem::List(list));
            }
            _ => return Err(p.fail("'NEWLINE', 'ACTOR', 'participant', 'note', 'end'")),
        }
    }
}
