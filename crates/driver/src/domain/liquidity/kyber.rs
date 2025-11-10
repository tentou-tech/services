use {
    crate::domain::{eth, liquidity},
    anyhow::Context,
    primitive_types::{H160, U256},
    shared::kyber_api::MethodParameters,
};

#[derive(Clone, Debug)]
pub struct KyberSwap {
    pub method: MethodParameters,
}

impl KyberSwap {
    pub fn to_interaction(&self, _input: &liquidity::MaxInput) -> anyhow::Result<eth::Interaction> {
        // Decode the calldata from build_swap response
        let call_data_hex = self.method.calldata.trim_start_matches("0x");
        let call_data = const_hex::decode(call_data_hex)
            .context("failed to decode Kyber calldata from build_swap")?;

        // Parse the target router address
        let target = self.method.to.trim_start_matches("0x");
        let target_bytes = const_hex::decode(target)
            .context("failed to decode Kyber router address")?;
        let target_address = H160::from_slice(&target_bytes);

        // Parse the value (usually 0 for token swaps)
        let value: U256 = self.method.value.parse()
            .unwrap_or(U256::zero());

        Ok(eth::Interaction {
            target: target_address.into(),
            value: value.into(),
            call_data: call_data.into(),
        })
    }
}
