use app::script::ScriptVariable;

pub fn parse(source: &str) -> Vec<ScriptVariable> {
    let mut variables = source
        .lines()
        .filter_map(|line| {
            let declaration = line.strip_prefix("declare ")?;
            let (_, declaration) = declaration.split_once(' ')?;
            let (name, value) = declaration.split_once('=').unwrap_or((declaration, ""));
            if name.starts_with("_Dbg_")
                || name.starts_with("BASH")
                || name.starts_with("__TUNDRA")
                || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return None;
            }
            Some(ScriptVariable {
                name: name.to_owned(),
                value: value.to_owned(),
                children: array_items(value),
            })
        })
        .collect::<Vec<_>>();
    variables.sort_by(|left, right| left.name.cmp(&right.name));
    variables
}

fn array_items(value: &str) -> Vec<ScriptVariable> {
    let Some(body) = value
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Vec::new();
    };
    let bytes = body.as_bytes();
    let mut result = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        while bytes
            .get(offset)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            offset += 1;
        }
        if bytes.get(offset) != Some(&b'[') {
            break;
        }
        let start = offset + 1;
        let Some(close) = key_end(bytes, start) else {
            break;
        };
        offset = close + 2;
        let value_start = offset;
        let mut quote = None;
        while offset < bytes.len() {
            match bytes[offset] {
                b'\\' => {
                    offset = (offset + 2).min(bytes.len());
                    continue;
                }
                byte @ (b'"' | b'\'') if quote.is_none() => quote = Some(byte),
                byte if quote == Some(byte) => quote = None,
                byte if byte.is_ascii_whitespace() && quote.is_none() => break,
                _ => {}
            }
            offset += 1;
        }
        result.push(ScriptVariable {
            name: body[start..close].to_owned(),
            value: body[value_start..offset].to_owned(),
            children: Vec::new(),
        });
    }
    result
}

fn key_end(bytes: &[u8], mut offset: usize) -> Option<usize> {
    let mut quote = None;
    while offset < bytes.len() {
        match bytes[offset] {
            b'\\' => {
                offset += 2;
                continue;
            }
            byte @ (b'"' | b'\'') if quote.is_none() => quote = Some(byte),
            byte if quote == Some(byte) => quote = None,
            b']' if quote.is_none() && bytes.get(offset + 1) == Some(&b'=') => return Some(offset),
            _ => {}
        }
        offset += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declarations_keep_escaping_and_expand_arrays() {
        let vars = parse(
            "declare -- x=\"a  b\\\\c\"\ndeclare -a a=([0]=\"中 文\" [1]=\"b\")\ndeclare -- _Dbg_secret=1\n",
        );
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].children[0].value, "\"中 文\"");
        assert_eq!(vars[1].value, "\"a  b\\\\c\"");
    }

    #[test]
    fn quoted_array_keys_and_ansi_c_values_keep_their_contents() {
        let vars = parse("declare -A names=([\"a]= b\"]=$'a b\\n' [other]=\"ok\")");
        assert_eq!(vars[0].children.len(), 2);
        assert_eq!(vars[0].children[0].name, "\"a]= b\"");
        assert_eq!(vars[0].children[0].value, "$'a b\\n'");
    }
}
