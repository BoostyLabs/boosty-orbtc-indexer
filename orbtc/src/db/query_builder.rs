use sqlx::{Database, QueryBuilder};

pub struct DynamicQueryBuilder<'a, DB: Database> {
    query: QueryBuilder<'a, DB>,
    has_where: bool,
}

impl<'a, DB: Database> DynamicQueryBuilder<'a, DB> {
    pub fn new(initial_query: &str) -> Self {
        Self {
            query: QueryBuilder::new(initial_query),
            has_where: false,
        }
    }

    pub fn add_and<T>(&mut self, condition: &str, value: Option<T>) -> &mut Self
    where
        T: 'a + Send + sqlx::Encode<'a, DB> + sqlx::Type<DB>,
    {
        if let Some(val) = value {
            if self.has_where {
                self.query.push(" AND ");
            } else {
                self.query.push(" WHERE ");
                self.has_where = true;
            }
            self.query.push(condition).push_bind(val);
        }
        self
    }

    pub fn query(&mut self) -> &mut QueryBuilder<'a, DB> {
        &mut self.query
    }
}
