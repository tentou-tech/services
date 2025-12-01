const { createWalletClient, http } = require('viem');
const { privateKeyToAccount, generatePrivateKey } = require('viem/accounts');
const { mainnet } = require('viem/chains');

// 1. Setup Client & Account
// const privateKey = generatePrivateKey();
const privateKey =
  '0x6f8381267819901534cf01edbb6763e816b38c57a34dda381c3a251de5a57c7d';
const account = privateKeyToAccount(privateKey);

const client = createWalletClient({
  account,
  chain: mainnet,
  transport: http(),
});

console.log(`Generated Signer Address (Owner): ${account.address}`);

// 2. Define EIP-712 Domain
const domain = {
  name: 'Gnosis Protocol',
  version: 'v2',
  chainId: 998,
  verifyingContract: '0xdc746a7ff2daaf182da82560318f6c1b36d067b1',
};

// 3. Define Types
const types = {
  Order: [
    { name: 'sellToken', type: 'address' },
    { name: 'buyToken', type: 'address' },
    { name: 'receiver', type: 'address' },
    { name: 'sellAmount', type: 'uint256' },
    { name: 'buyAmount', type: 'uint256' },
    { name: 'validTo', type: 'uint32' },
    { name: 'appData', type: 'bytes32' },
    { name: 'feeAmount', type: 'uint256' },
    { name: 'kind', type: 'string' },
    { name: 'partiallyFillable', type: 'bool' },
    { name: 'sellTokenBalance', type: 'string' },
    { name: 'buyTokenBalance', type: 'string' },
  ],
};

// 4. Define Message
const validTo = Math.floor(Date.now() / 1000) + 31536000; // +1 year

const message = {
  sellToken: '0xadcb2f358eae6492f61a5f87eb8893d09391d160',
  buyToken: '0xc003d79b8a489703b1753711e3ae9ffdfc8d1a82',
  receiver: account.address,
  sellAmount: 1000000000000000n, // 0.001 with 18 decimals
  buyAmount: 1000000000000000n, // 0.001 with 18 decimals
  validTo: validTo,
  appData: '0x2777e73a764bccd87db7421965088f9dffae9e67aa72caf85671af3c7d5f0f91',
  feeAmount: 0n,
  kind: 'sell',
  partiallyFillable: false,
  sellTokenBalance: 'erc20',
  buyTokenBalance: 'erc20',
};

async function signOrder() {
  try {
    const signature = await client.signTypedData({
      account,
      domain,
      types,
      primaryType: 'Order',
      message,
    });

    console.log('\n--- Signed Order Details ---');
    console.log('Copy these values into your solve_request.json:\n');
    console.log(`"validTo": ${validTo},`);
    console.log(`"receiver": "${account.address}",`);
    console.log(`"owner": "${account.address}",`);
    console.log(`"signature": "${signature}"`);
    console.log('----------------------------\n');
    console.log('Note: Ensure "feeAmount": "0" is added to the order object.');
  } catch (err) {
    console.error('Error signing order:', err);
  }
}

signOrder();
