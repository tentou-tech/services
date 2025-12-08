#!/bin/bash
set -e

# Start the driver in the background
echo "Starting driver..."
ETHRPC=http://localhost:8545 ADDR="0.0.0.0:9000" cargo run --bin driver-hyperliquid-template -- --config configs/local/driver.toml
# cargo run -p driver-hyperliquid-template -- \
#     --ethrpc "http://localhost:8545" \
#     --solver-account "0x1234567890123456789012345678901234567890123456789012345678901234" \
#     --solvers-config-file "/home/tuan1998/services/solvers_config.toml" \
#     --liquidity-strategy-config-file "/home/tuan1998/services/liquidity_strategy_config.toml" \
#     --order-sorting-strategy-config-file "/home/tuan1998/services/order_sorting_strategy_config.toml" &
DRIVER_PID=$!

# Wait for the driver to start
echo "Waiting for driver to start..."
sleep 10

# Call /solve to get a solutionId
echo "Calling /solve..."
SOLVE_RESPONSE=$(curl -s -X POST -H "Content-Type: application/json" -d @solve_request.json http://localhost:9586/solve)
echo "Solve Response: $SOLVE_RESPONSE"

# Extract solutionId (assuming jq is available, or use grep/sed)
# Since I don't know if jq is installed, I'll use python to extract it safely
SOLUTION_ID=$(echo $SOLVE_RESPONSE | python3 -c "import sys, json; print(json.load(sys.stdin)['solutions'][0]['solutionId'])")
echo "Solution ID: $SOLUTION_ID"

# Construct settle request
SETTLE_REQUEST=$(cat <<EOF
{
  "solutionId": $SOLUTION_ID,
  "auctionId": "1",
  "submissionDeadlineLatestBlock": 12345678
}
EOF
)

# Call /settle
echo "Calling /settle..."
SETTLE_RESPONSE=$(curl -s -w "%{http_code}" -X POST -H "Content-Type: application/json" -d "$SETTLE_REQUEST" http://localhost:9586/settle)
HTTP_CODE=${SETTLE_RESPONSE: -3}
BODY=${SETTLE_RESPONSE::-3}

echo "Settle Response Code: $HTTP_CODE"
echo "Settle Response Body: $BODY"

# Kill the driver
kill $DRIVER_PID

if [ "$HTTP_CODE" == "200" ]; then
  echo "Verification Successful!"
else
  echo "Verification Failed!"
  exit 1
fi
