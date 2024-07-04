use anchor_lang::AccountDeserialize;
use anyhow::Result;
use solana_sdk::account::Account;

pub fn deserialize_anchor_account<T: AccountDeserialize>(account: &Account) -> Result<T> {
    let mut data: &[u8] = &account.data;
    T::try_deserialize(&mut data).map_err(Into::into)
}
