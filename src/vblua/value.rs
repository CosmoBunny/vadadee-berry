//! `Lua Value <-> VBLua Value <-> Rust Value` conversion layer (Phase 6 groundwork).
//!
//! The boundary is deliberate: Lua never sees raw Rust pointers. IDs cross as
//! strings (`Uuid`), numeric vectors as tables, everything else as scalars.
//! Conversions are total where possible and return `VbluaError::Api` otherwise.

use super::error::VbluaError;

/// Canonical script-visible value. Every public API marshals through this.
#[derive(Debug, Clone, PartialEq)]
pub enum VbValue {
    Nil,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
    /// RGBA, components 0..=1.
    Color([f64; 4]),
    /// Stable entity reference — never a pointer. Uuid rendered as string.
    NodeRef(String),
    AssetRef(String),
    /// `{"name" = value, ...}` for enums / option bags.
    Map(Vec<(String, VbValue)>),
    List(Vec<VbValue>),
}

impl VbValue {
    /// Convert into an `mlua::Value` on the given state.
    pub fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
        use mlua::Value as L;
        Ok(match self {
            VbValue::Nil => L::Nil,
            VbValue::Bool(b) => L::Boolean(b),
            VbValue::Int(i) => L::Integer(i),
            VbValue::Num(n) => L::Number(n),
            VbValue::Str(s) => L::String(lua.create_string(&s)?),
            VbValue::Vec2([x, y]) => {
                let t = lua.create_table()?;
                t.set(1, x)?;
                t.set(2, y)?;
                L::Table(t)
            }
            VbValue::Vec3([x, y, z]) => {
                let t = lua.create_table()?;
                t.set(1, x)?;
                t.set(2, y)?;
                t.set(3, z)?;
                L::Table(t)
            }
            VbValue::Vec4(v) | VbValue::Color(v) => {
                let t = lua.create_table()?;
                for (i, c) in v.iter().enumerate() {
                    t.set(i + 1, *c)?;
                }
                L::Table(t)
            }
            VbValue::NodeRef(id) | VbValue::AssetRef(id) => {
                L::String(lua.create_string(&id)?)
            }
            VbValue::Map(pairs) => {
                let t = lua.create_table()?;
                for (k, v) in pairs {
                    t.set(k, v.into_lua(lua)?)?;
                }
                L::Table(t)
            }
            VbValue::List(items) => {
                let t = lua.create_table()?;
                for (i, v) in items.into_iter().enumerate() {
                    t.set(i + 1, v.into_lua(lua)?)?;
                }
                L::Table(t)
            }
        })
    }

    /// Best-effort read of an `mlua::Value`. Tables become `List` unless they
    /// carry string keys (then `Map`). Use the typed helpers (`as_vec2`, ...)
    /// when the shape matters.
    pub fn from_lua(value: mlua::Value, lua: &mlua::Lua) -> Result<Self, VbluaError> {
        use mlua::Value as L;
        let _ = lua;
        Ok(match value {
            L::Nil => VbValue::Nil,
            L::Boolean(b) => VbValue::Bool(b),
            L::Integer(i) => VbValue::Int(i),
            L::Number(n) => VbValue::Num(n),
            L::String(s) => VbValue::Str(
                s.to_str()
                    .map(|s| s.to_string())
                    .map_err(|e| VbluaError::Api {
                        api: "value".into(),
                        message: format!("invalid UTF-8 in Lua string: {e}"),
                    })?,
            ),
            L::Table(t) => {
                // Peek: any string key -> Map, else List.
                let mut saw_string_key = false;
                for pair in t.clone().pairs::<mlua::Value, mlua::Value>() {
                    if let Ok((k, _)) = pair
                        && matches!(k, L::String(_))
                    {
                        saw_string_key = true;
                        break;
                    }
                }
                if saw_string_key {
                    let mut pairs = Vec::new();
                    for pair in t.pairs::<String, mlua::Value>() {
                        let Ok((k, v)) = pair else { continue };
                        pairs.push((k.clone(), VbValue::from_lua(v, lua)?));
                    }
                    VbValue::Map(pairs)
                } else {
                    let mut items = Vec::new();
                    for v in t.sequence_values::<mlua::Value>() {
                        let Ok(v) = v else { continue };
                        items.push(VbValue::from_lua(v, lua)?);
                    }
                    VbValue::List(items)
                }
            }
            L::Function(_) | L::Thread(_) | L::UserData(_) | L::LightUserData(_) | L::Error(_) | L::Other(_) => {
                return Err(VbluaError::Api {
                    api: "value".into(),
                    message: "unsupported Lua value (function/thread/userdata cannot cross the VBLua boundary)".into(),
                });
            }
        })
    }

    /// Read a `{x,y} or [x,y]` table as `Vec2`.
    pub fn as_vec2(value: &mlua::Value) -> Option<[f64; 2]> {
        let t = value.as_table()?;
        let num = |k: &str, i: i64| -> Option<f64> {
            t.get::<f64>(k)
                .ok()
                .or_else(|| t.get::<f64>(i).ok())
                .or_else(|| t.get::<i64>(k).ok().map(|v| v as f64))
                .or_else(|| t.get::<i64>(i).ok().map(|v| v as f64))
        };
        Some([num("x", 1)?, num("y", 2)?])
    }

    /// Read a `{x,y,z} or [x,y,z]` table as `Vec3`.
    pub fn as_vec3(value: &mlua::Value) -> Option<[f64; 3]> {
        let t = value.as_table()?;
        let num = |k: &str, i: i64| -> Option<f64> {
            t.get::<f64>(k)
                .ok()
                .or_else(|| t.get::<f64>(i).ok())
                .or_else(|| t.get::<i64>(k).ok().map(|v| v as f64))
                .or_else(|| t.get::<i64>(i).ok().map(|v| v as f64))
        };
        Some([num("x", 1)?, num("y", 2)?, num("z", 3)?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    #[test]
    fn scalars_round_trip() {
        let lua = Lua::new();
        for v in [
            VbValue::Nil,
            VbValue::Bool(true),
            VbValue::Int(-7),
            VbValue::Num(0.5),
            VbValue::Str("hi".into()),
        ] {
            let lv = v.clone().into_lua(&lua).unwrap();
            assert_eq!(VbValue::from_lua(lv, &lua).unwrap(), v);
        }
    }

    #[test]
    fn vec2_table_reads_both_shapes() {
        let lua = Lua::new();
        let seq: mlua::Value = lua.load("return {10, 20}").eval().unwrap();
        assert_eq!(VbValue::as_vec2(&seq), Some([10.0, 20.0]));
        let named: mlua::Value = lua.load("return {x=1.5, y=-2.5}").eval().unwrap();
        assert_eq!(VbValue::as_vec2(&named), Some([1.5, -2.5]));
    }

    #[test]
    fn functions_are_rejected() {
        let lua = Lua::new();
        let f: mlua::Value = lua.load("return print").eval().unwrap();
        assert!(VbValue::from_lua(f, &lua).is_err());
    }
}
