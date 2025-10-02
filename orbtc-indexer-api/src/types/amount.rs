#[cfg(feature = "diesel")]
use bigdecimal::BigDecimal;

#[rustfmt::skip]
#[cfg(feature = "diesel")]
use diesel::{
    pg::Pg,
    backend::Backend,
    deserialize::{self, FromSqlRow, FromSql},
    expression::AsExpression,
    serialize::{self, Output, ToSql},
    sql_types::Numeric,
};

// #[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
// #[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Default, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "diesel", derive(FromSqlRow, AsExpression))]
#[cfg_attr(feature = "diesel", diesel(sql_type = Numeric))]
pub struct Amount(pub u128);

#[cfg(feature = "diesel")]
impl ToSql<Numeric, Pg> for Amount
where
    BigDecimal: ToSql<Numeric, Pg>,
{
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let v: BigDecimal = self.0.into();
        <BigDecimal as ToSql<Numeric, Pg>>::to_sql(&v, &mut out.reborrow())
    }
}

#[cfg(feature = "diesel")]
impl<DB> FromSql<Numeric, DB> for Amount
where
    DB: Backend,
    BigDecimal: FromSql<Numeric, DB>,
{
    fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
        use bigdecimal::ToPrimitive;

        let raw_am: BigDecimal = BigDecimal::from_sql(bytes)?;
        let am = raw_am.to_u128().unwrap();
        Ok(Self(am))
    }
}

impl serde::Serialize for Amount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Amount {
    fn deserialize<D>(deserializer: D) -> Result<Amount, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct BigDecimalVisitor;

        impl serde::de::Visitor<'_> for BigDecimalVisitor {
            type Value = Amount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a hex-encoded string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Amount, E>
            where
                E: serde::de::Error,
            {
                use std::str::FromStr;
                Ok(Amount(u128::from_str(value).map_err(E::custom)?))
            }
        }

        deserializer.deserialize_str(BigDecimalVisitor)
    }
}
