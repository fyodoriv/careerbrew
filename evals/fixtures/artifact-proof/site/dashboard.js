// Browser-context scope: closing the fresh test context deletes the review choice.
const behavior = __ARTIFACT_PROOF_BEHAVIOR__;
const value = document.querySelector('#value');
const reviewState = document.querySelector('#review-state');
const reviewKey = 'sample-reviewed';
let current = null;

const update = async () => {
  const response = await fetch('provider/data.json', { cache: 'no-store' });
  if (!response.ok) throw new Error(`Source returned ${response.status}`);
  current = (await response.json()).nonce;
  value.textContent = current;
};

const showReview = () => {
  reviewState.textContent = localStorage.getItem(reviewKey) === 'yes' ? 'Reviewed' : 'Not reviewed';
};

document.querySelector('#export').addEventListener('click', () => {
  if (!behavior.export) return;
  const blob = new Blob([JSON.stringify({ nonce: current }) + '\n'], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = 'sample-value.json';
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
});

document.querySelector('#review').addEventListener('click', async () => {
  const response = await fetch('receipt.json', { cache: 'no-store' });
  if (!response.ok) throw new Error(`Receipt returned ${response.status}`);
  if (behavior.review) localStorage.setItem(reviewKey, 'yes');
  showReview();
});

document.querySelector('#second-help').href = behavior.help;
showReview();
await update();
if (behavior.poll) setInterval(() => void update(), 1000);
