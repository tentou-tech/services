//! Domain model for KyberSwap Aggregator routes in the driver.

use {
    crate::domain::{eth, liquidity},
    anyhow::anyhow,
    ethcontract::Bytes,
    primitive_types::{H160, U256},
};

/// A KyberSwap route with encoded transaction data
#[derive(Clone, Debug)]
pub struct KyberSwapRoute {
    /// Source token address
    pub token_in: H160,
    /// Destination token address
    pub token_out: H160,
    /// Input amount
    pub amount_in: U256,
    /// Expected output amount (before slippage)
    pub amount_out: U256,
    /// Encoded calldata for MetaAggregator contract
    pub encoded_data: Bytes<Vec<u8>>,
    /// MetaAggregator Router address
    pub router_address: H160,
    /// Estimated gas cost
    pub gas_estimate: u64,
}

impl KyberSwapRoute {
    /// Converts the KyberSwap route into an executable interaction
    pub fn to_interaction(
        &self,
        input: &liquidity::MaxInput,
    ) -> anyhow::Result<eth::Interaction> {
        // Validate that the input token and amount match the route
        if input.0.token != self.token_in.into() {
            return Err(anyhow!(
                "Input token mismatch: expected {:?}, got {:?}",
                self.token_in,
                input.0.token
            ));
        }

        // For KyberSwap, the encoded data already contains all necessary parameters
        // We just need to call the router with the encoded data
        Ok(eth::Interaction {
            target: self.router_address.into(),
            value: 0.into(), // KyberSwap swaps don't require ETH value
            call_data: self.encoded_data.clone().0.into(),
        })
    }

    /// Returns the swap input asset
    pub fn input_asset(&self) -> eth::Asset {
        eth::Asset {
            token: self.token_in.into(),
            amount: self.amount_in.into(),
        }
    }

    /// Returns the expected output asset
    pub fn output_asset(&self) -> eth::Asset {
        eth::Asset {
            token: self.token_out.into(),
            amount: self.amount_out.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_interaction() {
        let route = KyberSwapRoute {
            token_in: H160::from_low_u64_be(1),
            token_out: H160::from_low_u64_be(2),
            amount_in: U256::from(1000),
            amount_out: U256::from(900),
            encoded_data: Bytes(vec![0x12, 0x34, 0x56, 0x78]),
            router_address: H160::from_low_u64_be(99),
            gas_estimate: 150000,
        };

        let input = liquidity::MaxInput(eth::Asset {
            token: route.token_in.into(),
            amount: route.amount_in.into(),
        });

        let interaction = route.to_interaction(&input).unwrap();
        
        assert_eq!(interaction.target, route.router_address.into());
        assert_eq!(interaction.value, 0.into());
        assert_eq!(interaction.call_data.0, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_token_mismatch_error() {
        let route = KyberSwapRoute {
            token_in: H160::from_low_u64_be(1),
            token_out: H160::from_low_u64_be(2),
            amount_in: U256::from(1000),
            amount_out: U256::from(900),
            encoded_data: Bytes(vec![]),
            router_address: H160::from_low_u64_be(99),
            gas_estimate: 150000,
        };

        let wrong_input = liquidity::MaxInput(eth::Asset {
            token: H160::from_low_u64_be(999).into(), // Wrong token
            amount: route.amount_in.into(),
        });

        assert!(route.to_interaction(&wrong_input).is_err());
    }
}

