# The epoch step

Status: **design, revised after an independent review. Stage 0 of the build
order is built; nothing else is.**

## In plain words

About once an hour every validator pauses for a few seconds, and together
they agree on a snapshot of every balance. The snapshot is a **checkpoint**.
The stretch between two checkpoints is an **epoch**.

This is the only moment the network needs everyone to agree on anything.
Payments still never wait in a line behind each other.

The checkpoint is what makes all of this possible:

- freeing an account that is stuck because two payments were signed with
  one key
- people joining and leaving as validators by staking coins
- paying fees to validators
- creating coins and sharing them out by work
- replacing validator keys before they run out
- discarding old receipts that the checkpoint has made redundant

What changes for someone using the network:

1. **"Final" takes two steps instead of one.** Today a payment is final when
   validators holding more than two thirds of the stake vote for it. After
   this change they must also confirm that they recorded it. The wallet
   already waits for both, so the time to finality stays about the same. A
   receipt grows from 5.5 KB to about 12 KB.
2. **Payments pause for a few seconds at each checkpoint.**
3. **An interrupted payment survives checkpoints.** It stays finishable, as
   it is now.
4. **A double-signed account is freed at the next checkpoint.** The payments
   signed with the burned key are cancelled, and the account carries on
   with a spare key.

The network does gain one small chain: the checkpoints, each naming the one
before. No payment ever waits for it.

## Why "final" needs a second step

Take four validators, where three votes make a certificate. An owner signs
two payments, A and B, under the same number. Validators 1, 3 and 4 vote
for A, so a certificate for A exists, but the wallet holding it shows it to
nobody. Validator 2 voted for B.

Now the epoch closes with validators 1, 2 and 3 taking part, and validator 3
is dishonest and stays silent about its vote. The others see one vote for A
and one for B. They cannot tell whether A has a certificate. If they cancel
both, a certificate that was "final" has just been undone. If they refuse to
cancel, the account stays stuck, which is the problem we set out to fix.

The way out is to say a payment is final only once validators holding more
than two thirds of the stake have confirmed they recorded its certificate.
Any group large enough to close the epoch then contains an honest validator
that recorded it, and the checkpoint is built from what that group
recorded.

Sui, the nearest large network to this design, uses the same rule.

## Rules

### 1. Epochs

Epochs are numbered from 0. Epoch `e` uses the validator set and settings
fixed by checkpoint `e-1`; epoch 0 uses the genesis file. An epoch ends an
hour after the previous checkpoint, or sooner once it holds a set number of
payments, so that no checkpoint has too much to reconcile.

### 2. Votes, certificates, confirmations

- A **vote** is a validator's signature over the network id, the epoch
  number and a batch root, where the batch contains the order's digest.
  Votes from a closed epoch count for nothing.
- A **certificate** is the owner's signed order, an epoch number, and votes
  from validators holding more than two thirds of that epoch's stake. It
  keeps the owner's signature, which today's certificate drops, so that a
  payment undone at a checkpoint can be voted on again.
- A validator that receives a valid certificate for the current epoch
  applies it, writes it to disk, and signs a **confirmation** for exactly
  that order. Confirmations go into the same batch as votes under a
  different tag, so one signature still covers the whole batch.
- A payment is **final** when confirmations from more than two thirds of
  the stake exist, from the same epoch as the votes. A receipt is the
  certificate plus those confirmations.

### 3. Promises outlive the epoch, votes do not

- A validator that voted for an order keeps the signed order and its
  promise not to vote for a different one under that account, number and
  key. The promise holds across checkpoints, until a checkpoint advances
  that number or cancels it.
- The vote itself expires with its epoch. In a new epoch votes are
  collected afresh, and each validator will only vote again for the order
  it is promised to.
- **Every vote re-checks everything**: signature, number, and balance
  against the validator's current state. A repeated vote is no exception.
  Today's code skips the balance check on a repeat, which would let a
  payment undone at a checkpoint leave spendable coins behind.

So nothing about unfinished payments needs to be agreed at a checkpoint. An
interrupted payment simply stays finishable by anyone holding the signed
order.

### 4. Evidence of double-signing

A validator that is shown a second, different order validly signed with an
account's current key under its current number keeps both signed orders,
passes them to the other validators, and votes for neither from then on.
Two such orders are **evidence**. Nobody can fabricate evidence against an
owner who signed once.

### 5. Closing an epoch

1. **Start.** A validator starts closing when its own clock passes the end
   of the epoch, when the payment limit is reached, or when it has closing
   statements from validators holding more than one third of the stake.
   Dishonest validators alone cannot start a close.
2. **Freeze.** It finishes the batch it has open, then votes on no order
   and signs no confirmation in this epoch. It keeps recording orders and
   evidence it is shown.
3. **Statement.** It signs one statement that pins its certificate log for
   the epoch (its length and a running hash, so the content cannot be
   changed afterwards), lists the evidence it holds, and names a
   replacement key tree if it wants one. The statement is written to disk
   before it is sent.
4. **Reconcile.** Validators pull each other's logs up to the pinned
   points. A certificate received now is applied but not confirmed.
5. **Propose.** The proposer merges the pinned logs of validators holding
   more than two thirds of the stake, waits briefly for the rest, and
   proposes a checkpoint together with the statements it merged.
6. **A proposed checkpoint is valid when**, and these tests give every
   validator the same answer:
   - the merged statements cover more than two thirds of the stake;
   - the checkpoint's payments include every certificate in each merged
     statement's pinned log, and every certificate verifies for this epoch;
   - replaying those payments on the previous checkpoint's state, then
     applying cancellations and payouts by the fixed rules, gives the
     checkpoint's state root;
   - no balance is negative;
   - each cancellation comes with evidence that holds against the
     resulting state (rule 6);
   - its time is close to the validator's own clock.
7. **Agree.** One round-based agreement in the style of Tendermint, with
   its rules unchanged: a proposer chosen in rotation by stake, prevotes,
   precommits, a lock on precommit, timeouts that grow each round. A
   checkpoint that already has prevotes from more than two thirds is
   accepted as it stands when proposed again.
8. **Result.** A checkpoint with precommits from more than two thirds of
   the stake is decided. Every validator switches, in one atomic step, to
   the checkpoint's state and an empty log for the new epoch. Anything it
   had applied that the checkpoint leaves out is undone, and the signed
   order behind it goes back to being an unfinished promise.

If fewer than two thirds of the stake are online, the close cannot finish
and the network stays paused. Payments could not become final in that
situation anyway.

### 6. Cancellation and spare keys

- Every address commits to two things: the first key and a **spare tree**,
  a small set of one-time keys under one root. The first payment reveals
  the root.
- A checkpoint cancels an account's current number when it is given
  evidence (rule 4) that still holds after the checkpoint's payments are
  applied. If one of the two orders became final, the number has advanced
  and the evidence no longer applies.
- After a cancellation the account keeps its number and its next payment
  must be signed with the next unused key of the spare tree. That payment
  names the following ordinary key.
- The spare tree was fixed before the burned key was ever used, so someone
  who can forge the burned key has no hold on it.
- A second double-signing uses up the next spare. An account that exhausts
  its spare tree stays locked.
- A network outage never costs anyone a key. Only signing twice does.

### 7. The checkpoint

- The **header** is small: epoch number, digest of the previous
  checkpoint, time, state root, the validator set and settings for the
  next epoch, and totals for fees and released coins. Wallets keep headers.
- The **body** is the cancellations with their evidence. Payments are not
  listed: validators already hold the certificates, and the state root
  pins the result.
- The state root is a Merkle root over all accounts in address order, so a
  wallet can be given a short proof of its own account against a header.

### 8. What a wallet may believe

- A wallet advances its payment number only on a certificate with
  confirmations from the same epoch, or on a checkpoint header with a
  proof of its account. Until then it keeps the signed order.
- A wallet moves to a spare key only on a header with a proof that its
  account was cancelled. A single validator's word is not enough.
- After a cancellation a wallet never retries by itself. Two copies doing
  that would sign twice again.

### 9. What a validator must never forget

Each of these is on disk before the message that depends on it leaves:

- where its certificate log stood when it froze;
- its closing statement;
- every proposal, prevote and precommit it sends;
- the decided checkpoint and the precommits that decided it.

State is kept as one snapshot and one log per epoch, so that a restart
after a checkpoint cannot replay payments the checkpoint left out. A
validator that restarts mid-close sends the same messages again, signed
with fresh keys.

### 10. Time and keys

- **Time.** A validator checks a new proposal's time against its own
  clock. Coins are released according to the time that has passed between
  checkpoints, not per checkpoint, so closing early or late does not change
  how many coins exist.
- **Keys.** A validator stops voting on payments when its key tree is down
  to a reserve kept for closing messages. A replacement tree named in its
  statement takes over at the checkpoint.

### 11. Why it is safe (sketch)

- **Validators agree.** This rests on the locks and on votes written to
  disk before they are sent. It does not depend on what a validator
  believes is valid, and validity is the same test for everyone.
- **Nothing final is lost.** A final payment was confirmed by more than two
  thirds of the stake. A checkpoint merges statements from more than two
  thirds. The two groups share an honest validator, which confirmed before
  it froze and pinned a log that contains the payment.
- **No two conflicting payments are both final.** Within an epoch that
  would need an honest validator to vote twice. Across epochs, old votes
  count for nothing and promises carry over.
- **A certificate from an honest payer always completes.** Its order stays
  promised at the validators that voted for it and can be voted again.
  Cancelling needs a second signature, which an honest payer never makes.
- **Coins are never created by a checkpoint.** Every vote re-checks the
  balance, and a checkpoint with a negative balance is invalid.

## The order format, fixed once

A signed order that is not yet final cannot be re-signed, so a later change
to the format would strand it. Stage 1b therefore fixes everything later
stages need:

- a **kind**: payment, stake, unstake, and room for more;
- the **largest fee** the owner accepts, so a fee change cannot strand it;
- the **next key**;
- the **spare root**, on an account's first order.

## Built on top of the close

### Staking and the validator set

- A stake order locks coins against a validator's key-tree root and
  address. An unstake order starts a waiting period of several epochs.
- The validator set for the next epoch is fixed in the checkpoint from the
  stakes recorded in it, with a minimum stake and a maximum number of
  validators. A newcomer learns the current validators and their addresses
  by following the checkpoints from the genesis file.
- Two votes for different orders under one number within one epoch, or two
  signatures from one key, cost a validator its stake at the next
  checkpoint.
- Delegating stake to someone else's validator is left for later.

### Fees

A flat fee per payment is taken from the sender when the payment is
applied. The fees of an epoch are paid out in its checkpoint to the
validators, in proportion to stake.

### Coin creation

The rules below were accepted on 2026-11-14.

- The genesis file allocates nothing. The cap is 100 million coins, each
  divisible to nine decimal places.
- Coins are released on a curve with a half-life of four years: about
  1,977 coins in the first hour.
- A **work share** is an account, an epoch and a number whose hash together
  with the epoch's seed falls below a target. Validators accept shares
  until they freeze and pass them to each other. A statement pins its
  validator's shares the way it pins certificates, so the tally is a fixed
  result of the merged statements and no proposer can tilt it.
- The checkpoint pays the coins released since the last one in proportion
  to shares. The target moves to keep the number of shares near a set
  figure.
- The first seed is the hash of a Bitcoin block at a height announced in
  advance. Each later seed comes from the previous checkpoint.
- The launch validators hold seats that carry voting weight, cannot be
  spent and earn no released coins, and fade out.

## Build order

Each stage can be built and tested on its own.

| Stage | What | Where |
|---|---|---|
| 0, built | No change to the protocol. A state root, account proofs and snapshots; one folder per epoch with an atomic switch; evidence kept when a second order is refused; the balance check on every vote; a key reserve. | `crates/ledger/src/state.rs`, `crates/crypto/src/merkle.rs`, `crates/net/src/store.rs`, `crates/net/src/node.rs` |
| 1a | Epoch number (always 0 for now) in votes and certificates. Certificates that keep the owner's signature. Signed confirmations. Receipts that carry them. A wallet that advances only on proof. | `crates/ledger`, `crates/net`, `crates/cli` |
| 1b | The order format, fixed once: kind, largest fee, spare tree, the new address form. Cancellation as a pure function. The wallet's key streams. | `crates/ledger`, `crates/net/src/keys.rs`, `crates/crypto/src/hash.rs` |
| 2 | The close as a pure state machine with no network or disk, and a simulator that runs it under delays, crashes and dishonest validators. The failures the review found become permanent tests. | new files in `crates/ledger` |
| 3 | The close inside the validator with a fixed validator set: freeze, messages between validators, what is written to disk, the switch of state, the checkpoint chain, account proofs, key-tree replacement. | `crates/net`, `crates/cli` |
| 4 | Staking, a changing validator set, penalties. | all crates |
| 5 | Fees. | `crates/ledger`, `crates/net` |
| 6 | Coin creation, a genesis with no coins, and a command that does the work. | all crates |

Stuck accounts are freed from stage 3 on.

## Not decided

- **A slow sunrise.** Whether the release rate starts near zero and climbs
  to full over the first 90 days.
- **How launch seats fade.** If they fade by the calendar while little real
  stake exists, a third of the voting weight becomes cheap to buy. Tying
  the fade to the amount of real stake avoids that.
- **The size of the fee**, and whether it can change over time.
- **Limits on the validator set**: the minimum stake, the maximum number of
  validators, and the unstake waiting period.
- **The payment limit per epoch.**

## What the first draft got wrong

An independent review, asked to break the first draft, found these. Each is
fixed above.

- **The close could freeze the network forever with no dishonest
  validator.** The draft let each validator reject a checkpoint for reasons
  only it knew, such as a payment only it had seen. Two validators could
  lock onto a checkpoint a third would never accept. Validity is now the
  same test for everyone, built from signed statements (rule 5).
- **A carried payment could create coins.** A payment undone at a
  checkpoint could leave a later payment that depended on it votable with
  no balance check. Every vote now re-checks the balance, and a negative
  balance makes a checkpoint invalid (rules 3 and 5).
- **The proposer had choices it should not have.** It could pick which of
  two double-signed payments survived, and could get a forged one carried.
  Nothing is carried through agreement any more; checkpoints hold
  cancellations only (rules 3 and 6).
- **A checkpoint could be too big to send**, after an hour of payments at
  full speed. Payments are no longer listed, and an epoch also ends on a
  payment count (rules 1 and 7).
- **The proposer chose the time unchecked**, which could stall the network
  or speed up the release of coins (rule 10).
- **Validator keys could run out before the step that replaces them**
  (rule 10, and stage 3 of the build order).
- **A second double-signing had no defined outcome.** The spare is now a
  small tree of keys (rule 6).
- **The wallet trusted a bare certificate**, and one lying validator could
  have misled it (rule 8).
- **A restart after a checkpoint would have replayed undone payments**
  (rule 9).

The review also confirmed the core: validators cannot disagree, nothing
final is lost, no two conflicting payments both become final, and an owner
who signs once can never be cancelled. It judged the alternative of a
four-fifths quorum to be worse for this network.
