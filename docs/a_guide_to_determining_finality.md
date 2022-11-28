# A guide to determining finality



https://github.com/godwokenrises/godwoken/pull/836 changes the way to determine finality. The following guide is for dApp developers to help them understand what has changed and how to adapt.



## Changes

### Introduce __`timepoint`__ concept

A `timepoint` is a type, underlying `u64`, that is interpretated according to its highest bit.

- When the highest bit is `0`, the rest bits are represented by block number
- When the highest bit is `1`, the rest bits are represented by block timestamp



Here are some examples:

| timepoint full value  | timepoint full value in binary                               | Interpretation             |
| --------------------- | ------------------------------------------------------------ | -------------------------- |
| `0`                   | `b0000000000000000000000000000000000000000000000000000000000000000` | `BlockNumber(0)`           |
| `7489999`             | `b0000000000000000000000000000000000000000011100100100100111001111` | `BlockNumber(7489999)`     |
| `9223372036862265807` | `b1000000000000000000000000000000000000000011100100100100111001111` | `BlockTimestamp (7489999)` |
| `9223372036854775808` | `b1000000000000000000000000000000000000000000000000000000000000000` | `BlockTimestamp(0)`        |

### Interpretation of `WithdrawalLockArgs.withdrawal_block_timepoint` changed

>  `WithdrawalLockArgs.withdrawal_block_number` was renamed to `WithdrawalLockArgs.withdrawal_block_timepoint`.



For dApps that estimate the pending time of withdrawals:

- Previously, it should interpret `WithdrawalLockArgs.withdrawal_block_number` as block number, and estimate the pending time of a withdrawal as follows:

  ```rust
  # current_tip_number is the L2 tip block number
  # ESTIMATE_BLOCK_INTERVAL is a hardcoded or calculated block avarage L2 block interval
  
  estimated_pending_time = (rollup_config.finality_blocks - (current_tip_number - WithdrawalLockArgs.withdrawal_block_number)) * ESTIMATE_BLOCK_INTERVAL;
  ```

- To apply this change, it should interpret `WithdrawalLockArgs.withdrawal_block_timepoint` as timepoint and estimate the pending time of a withdrawal as follows:

  ```rust
  # current_tip_number is the L2 tip block number
  # current_tip_timestamp is the L2 tip block timestamp
  # ESTIMATE_BLOCK_INTERVAL is a hardcoded or calculated block avarage L2 block interval
  
  const MASK: u64 = 1 << 63;
  let flag_bit = WithdrawalLockArgs.withdrawal_block_timepoint & MASK;
  let value = WithdrawalLockArgs.withdrawal_block_timepoint & (!MASK);
  if flag_bit == 0 {
    // flat_bit is 0, interprets by block number, use the old way
    estimated_pending_time = (rollup_config.finality_blocks - (current_tip_number - value)) * ESTIMATE_BLOCK_INTERVAL;
  } else {
    // flat_bit is 1, interprets by block timestamp
    estimated_pending_time = rollup_config.finality_blocks * ESTIMATE_BLOCK_INTERVAL - (current_tip_timestamp - value)
  }
  ```
