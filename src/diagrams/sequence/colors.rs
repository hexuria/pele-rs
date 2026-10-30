// Port of src/diagrams/sequence/colors.ts. Mermaid asks the browser whether the first word of a
// box title is a color. Pele has no browser, so it knows the color keywords and the rgb() and
// hsl() notations. Other functions, such as calc() and var(), are not recognized.

use crate::js;

const KEYWORDS: &str = "aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood \
cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray \
darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen \
darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue \
firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew \
hotpink indianred indigo ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan \
lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray \
lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine mediumblue \
mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise mediumvioletred \
midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid \
palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple \
rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue \
slateblue slategray slategrey snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white \
whitesmoke yellow yellowgreen transparent currentcolor inherit initial unset revert \
accentcolor accentcolortext activetext buttonborder buttonface buttontext canvas canvastext field fieldtext \
graytext highlight highlighttext linktext mark marktext selecteditem selecteditemtext visitedtext \
activeborder activecaption appworkspace background buttonhighlight buttonshadow captiontext inactiveborder \
inactivecaption inactivecaptiontext infobackground infotext menu menutext scrollbar threeddarkshadow threedface \
threedhighlight threedlightshadow threedshadow window windowframe windowtext";

#[derive(Clone, Copy, PartialEq)]
enum K {
    Bad,
    Number,
    Percent,
    Angle,
    None,
}

// RE_TERM /^[+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:e[+-]?\d+)?(%|deg|grad|rad|turn)?$/i
fn kind(term: &str) -> K {
    if term.to_lowercase() == "none" {
        return K::None;
    }
    let b = term.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let s = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - s
    };
    if digits(&mut i) > 0 {
        if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
            i += 1;
            digits(&mut i);
        }
    } else if i < b.len() && b[i] == b'.' {
        i += 1;
        if digits(&mut i) == 0 {
            return K::Bad;
        }
    } else {
        return K::Bad;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if digits(&mut j) > 0 {
            i = j;
        }
    }
    let unit = term[i..].to_ascii_lowercase();
    match unit.as_str() {
        "" => K::Number,
        "%" => K::Percent,
        "deg" | "grad" | "rad" | "turn" => K::Angle,
        _ => K::Bad,
    }
}

fn is_alpha(term: &str, modern: bool) -> bool {
    let k = kind(term);
    k == K::Number || k == K::Percent || (modern && k == K::None)
}

fn is_function(color: &str) -> bool {
    let hsl = color.starts_with("hsl");
    let Some(open) = color.find('(') else { return false };
    let name = &color[..open];
    if !matches!(name, "rgb" | "rgba" | "hsl" | "hsla") {
        return false;
    }
    if js::len16(color) > 120 || color.find(')') != Some(color.len() - 1) {
        return false;
    }
    let inner = js::trim(&color[open + 1..color.len() - 1]);
    let legacy = inner.contains(',');
    let mut parts: Vec<String>;
    let mut alpha: Option<String> = None;
    if legacy {
        parts = inner.split(',').map(|s| js::trim(s).to_string()).collect();
        if parts.len() == 4 {
            alpha = parts.pop();
        }
    } else {
        let slash: Vec<&str> = inner.split('/').collect();
        if slash.len() > 2 {
            return false;
        }
        // slash[0].trim().split(/\s+/)
        let head = js::trim(slash[0]);
        parts = if head.is_empty() { vec![String::new()] } else { head.split(js::is_space).filter(|s| !s.is_empty()).map(str::to_string).collect() };
        if slash.len() == 2 {
            alpha = Some(js::trim(slash[1]).to_string());
        }
    }
    if parts.len() != 3 || alpha.as_deref().is_some_and(|a| !is_alpha(a, !legacy)) {
        return false;
    }
    let kinds: Vec<K> = parts.iter().map(|p| kind(p)).collect();
    if legacy {
        if hsl {
            return (kinds[0] == K::Number || kinds[0] == K::Angle) && kinds[1] == K::Percent && kinds[2] == K::Percent;
        }
        return (kinds[0] == K::Number || kinds[0] == K::Percent) && kinds[1] == kinds[0] && kinds[2] == kinds[0];
    }
    for (i, &k) in kinds.iter().enumerate() {
        let ok = k == K::Number || k == K::None || if hsl && i == 0 { k == K::Angle } else { k == K::Percent };
        if !ok {
            return false;
        }
    }
    true
}

pub fn is_css_color(color: &str) -> bool {
    if color.contains('(') {
        return is_function(color);
    }
    let lower = color.to_lowercase();
    KEYWORDS.split(' ').any(|k| k == lower)
}
