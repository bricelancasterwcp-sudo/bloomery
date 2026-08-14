use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Budget {
    granted_tokens: u64,
    spent_tokens: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BudgetExhausted {
    pub remaining: u64,
    pub requested: u64,
}

impl Budget {
    pub fn new(granted_tokens: u64) -> Self {
        Self {
            granted_tokens,
            spent_tokens: 0,
        }
    }

    pub fn remaining(&self) -> u64 {
        self.granted_tokens.saturating_sub(self.spent_tokens)
    }

    pub fn check(&self, requested: u64) -> Result<(), BudgetExhausted> {
        let rem = self.remaining();
        if rem < requested {
            Err(BudgetExhausted {
                remaining: rem,
                requested,
            })
        } else {
            Ok(())
        }
    }

    pub fn charge(&mut self, actual: u64) {
        self.spent_tokens += actual;
    }

    pub fn spent(&self) -> u64 {
        self.spent_tokens
    }

    pub fn granted(&self) -> u64 {
        self.granted_tokens
    }
}
