pub trait Utxo: Clone + std::fmt::Debug {
    fn get_amount(&self) -> u128;
}

#[derive(Debug, thiserror::Error)]
pub enum KnapsackError {
    #[error("Not enough balance. Available: {available}, Required: {target}")]
    NotEnoughBalance { available: u128, target: u128 },
}

/// Finds the minimum number of UTXOs required to reach the target amount by
/// doing a binary search of the "next greater than" elements. Not
/// exactly a knapsack algorithm, but a simpler version that works for our use case.
/// Generic - can be used for both RUNE UTXOs and BTC UTXOs.
/// O(nlogn) time complexity.
/// INVARIANT: utxos MUST be sorted in descending order by `get_amount()`.
pub fn min_utxos_to_reach_target<U: Utxo>(
    utxos: &[U],
    target: u128,
) -> Result<Vec<U>, KnapsackError> {
    if utxos.is_empty() {
        return Err(KnapsackError::NotEnoughBalance {
            available: 0,
            target,
        });
    }

    debug_assert_ne!(target, 0, "Target amount is zero");

    let mut result: Vec<U> = Vec::new();
    let mut collected = 0;
    let mut idx = 0;

    while collected < target {
        let subset = &utxos[idx..];
        if subset.is_empty() {
            // no more UTXOs... fail with not enough balance
            return Err(KnapsackError::NotEnoughBalance {
                available: collected,
                target,
            });
        }

        let utxo = if subset[0].get_amount() >= target - collected {
            // if subset[0] is smaller than target, then there is no need to use binary search to find
            // the next GE element, it will always be subset[0].
            match binary_search_next_ge_than(subset, target - collected) {
                Some(found) => {
                    // found UTXO that is GE than target, we pick it.
                    idx = found;
                    &subset[found]
                }
                None => {
                    // not found any UTXO that is GE than target. In this case we append fist UTXO (it is the biggest).
                    idx += 1;
                    &subset[0]
                }
            }
        } else {
            // optimization: if subset[0] is already bigger than target, we can just pick it, as it is the biggest.
            idx += 1;
            &subset[0]
        };

        // pick this utxo
        collected += utxo.get_amount();
        result.push(utxo.clone());
    }

    Ok(result)
}

/// finds the index of the first element greater than the target.
/// `arr` must be sorted in descending order.
fn binary_search_next_ge_than<U: Utxo>(arr: &[U], target: u128) -> Option<usize> {
    debug_assert!(!arr.is_empty());
    match arr.binary_search_by(|val| {
        if val.get_amount() > target {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    }) {
        // found exactly
        Ok(index) => Some(index),
        // not found exactly
        Err(index) => {
            // check if utxo at `index` is GE than target, if so return it.
            if index < arr.len() && arr[index].get_amount() >= target {
                return Some(index);
            }

            // check if maybe there is one previous UTXO that is GE than target.
            // if so, return it.
            if index > 0 && arr[index - 1].get_amount() >= target {
                return Some(index - 1);
            }

            None
        }
    }
}

#[cfg(test)]
mod knapsack_tests {
    use rstest::rstest;

    use super::*;

    // Dummy implementation of the Utxo trait for testing purposes
    #[derive(Clone, Debug)]
    struct DummyUtxo {
        amount: u128,
    }

    impl Utxo for DummyUtxo {
        fn get_amount(&self) -> u128 {
            self.amount
        }
    }

    // Helper to create a descending sorted vector of DummyUtxo
    fn dummy_utxos() -> Vec<DummyUtxo> {
        vec![
            DummyUtxo { amount: 100 },
            DummyUtxo { amount: 80 },
            DummyUtxo { amount: 60 },
            DummyUtxo { amount: 40 },
            DummyUtxo { amount: 20 },
        ]
    }

    #[rstest]
    #[case(90, Some(0))]
    #[case(70, Some(1))]
    #[case(105, None)]
    #[case(100, Some(0))]
    #[case(20, Some(4))]
    #[case(1, Some(4))]
    fn test_binary_search_next_ge_than(#[case] target: u128, #[case] expected: Option<usize>) {
        let utxos = dummy_utxos();
        let res = binary_search_next_ge_than(&utxos, target);
        assert_eq!(res, expected);
    }

    #[test]
    fn test_single_element_vector() {
        let utxos = vec![DummyUtxo { amount: 100 }];
        // With a single element greater than target:
        let res = binary_search_next_ge_than(&utxos, 50);
        assert_eq!(res, Some(0));
        // With a target equal to the only element, we expect Some(0)
        let res2 = binary_search_next_ge_than(&utxos, 100);
        assert_eq!(res2, Some(0));
        // With a target higher than the only element, we expect None
        let res3 = binary_search_next_ge_than(&utxos, 101);
        assert_eq!(res3, None);
    }

    #[ignore = "Fix this"]
    #[test]
    fn test_target_zero_returns_empty_vec() {
        let utxos = vec![DummyUtxo { amount: 100 }, DummyUtxo { amount: 50 }];
        let _res = min_utxos_to_reach_target(&utxos, 0);
        // assert!(matches!(res, Err(KnapsackError::ZeroTarget)));
    }

    // Test: one or multiple UTXOs are required to meet the target.
    #[rstest]
    #[case(1, vec![20])] // minimal utxo is 20
    #[case(20, vec![20])] // single 20 is exactly the target
    #[case(21, vec![30])] // 20 no longer satisfies the target, so single 30 is required
    #[case(31, vec![40])] // 30 no longer satisfies the target, so single 40 is required
    #[case(41, vec![50])] // 40 no longer satisfies the target, so single 50 is required
    #[case(51, vec![50, 20])] // 50 no longer satisfies the target, so we pick 50 and for remaining 1 - we pick 20.
    #[case(70, vec![50, 20])] // exact
    #[case(71, vec![50, 30])]
    #[case(90, vec![50, 40])] // exact
    #[case(100, vec![50, 40, 20])]
    #[case(110, vec![50, 40, 20])]
    #[case(120, vec![50, 40, 30])] // exact
    #[case(130, vec![50, 40, 30, 20])]
    #[case(140, vec![50, 40, 30, 20])] // exact
    fn test_min_utxos_to_reach_target_multiple_utxos_satisfies_target(
        #[case] target: u128,
        #[case] expected: Vec<u128>,
    ) {
        // UTXOs sorted in descending order.
        let utxos = vec![
            DummyUtxo { amount: 50 },
            DummyUtxo { amount: 40 },
            DummyUtxo { amount: 30 },
            DummyUtxo { amount: 20 },
        ];
        let res = min_utxos_to_reach_target(&utxos, target);
        assert!(res.is_ok());
        let solution = res.unwrap();
        let amounts = solution
            .iter()
            .map(|u| u.get_amount())
            .collect::<Vec<u128>>();
        assert_eq!(amounts, expected);

        let total: u128 = solution.iter().map(|u| u.get_amount()).sum();
        assert!(total >= target);
    }

    // Test: insufficient balance returns a KnapsackError::NotEnoughBalance.
    #[rstest]
    #[case(100, 30+20+10)]
    #[case(30+20+10+1, 30+20+10 )]
    fn test_min_utxos_to_reach_target_insufficient_balance_returns_error(
        #[case] actual_target: u128,
        #[case] expected_available: u128,
    ) {
        // UTXOs sorted in descending order.
        let utxos = vec![
            DummyUtxo { amount: 30 },
            DummyUtxo { amount: 20 },
            DummyUtxo { amount: 10 },
        ];
        let res = min_utxos_to_reach_target(&utxos, actual_target);
        assert!(res.is_err());
        if let Err(KnapsackError::NotEnoughBalance { available, target }) = res {
            assert_eq!(available, expected_available);
            assert_eq!(target, actual_target);
        } else {
            panic!("Expected NotEnoughBalance error");
        }
    }

    #[ignore = "Fix this"]
    #[test]
    fn test_min_utxos_to_reach_target_zero_target() {
        let utxos = vec![
            DummyUtxo { amount: 30 },
            DummyUtxo { amount: 20 },
            DummyUtxo { amount: 10 },
        ];
        let res = min_utxos_to_reach_target(&utxos, 0);
        assert!(res.is_err());
        // match res {
        //     Err(KnapsackError::ZeroTarget) => (),
        //     _ => panic!("Expected ZeroTarget error"),
        // }
    }

    // Test: empty UTXOs returns a KnapsackError::NotEnoughBalance.
    #[test]
    fn test_min_utxos_to_reach_target_empty_utxos_returns_error() {
        let utxos: Vec<DummyUtxo> = vec![];
        let target = 1;
        let res = min_utxos_to_reach_target(&utxos, target);
        assert!(res.is_err());
        match res {
            Err(KnapsackError::NotEnoughBalance { available, target }) => {
                assert_eq!(available, 0);
                assert_eq!(target, 1);
            }
            _ => panic!("Expected an error"),
        }
    }
}
