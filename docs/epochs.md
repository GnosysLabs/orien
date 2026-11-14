# The epoch step

Status: **stages 0 to 4 of the build order are built. Checkpoints run, a
double-signed account is freed at the next one, and the validators of
each epoch are chosen by stake, with a penalty for cheating. Fees and coin
creation are designed here and not built.**

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
2. **Payments pause briefly at each checkpoint.** Measured: about a fifth
   of a second with every validator on one machine, and about a second
   across three machines with one of them 57 ms away.
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
- A validator signs a confirmation when it applies the certificate, and
  later only while that payment is still the account's latest. Once an
  account has paid again, a quorum must already have recorded the earlier
  payment in order to vote for the next, so nobody needs it confirmed. This
  also limits how many keys replayed old certificates can use up.

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
   points. A certificate received now is applied but not confirmed, and it
   is not part of the validator's pinned log.
5. **Propose.** The proposer merges the pinned logs of validators holding
   more than two thirds of the stake, waits briefly for the rest, and
   proposes a checkpoint together with the statements it merged. The epoch
   holds exactly the payments in those pinned logs. A checkpoint is
   therefore settled by the statements it names and its time, and the
   proposer has no other choices to make.
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
   its rules unchanged: a proposer chosen in proportion to stake, prevotes,
   precommits, a lock on precommit, timeouts that grow each round. A
   checkpoint that already has prevotes from more than two thirds is
   accepted as it stands when proposed again. Two messages carry their own
   proof and are judged on it alone: a checkpoint proposed again comes with
   the earlier prevotes that back it, and a decision comes with the
   precommits that made it. A validator that learns of a decision passes
   that proof on. The first round begins when statements from more than
   two thirds of the stake are in. That moment is nearly the same for every
   validator even when their clocks differ, so the rounds' waits are not
   used up by a clock that runs fast.
8. **Result.** A checkpoint with precommits from more than two thirds of
   the stake is decided. Every validator switches, in one atomic step, to
   the checkpoint's state and an empty log for the new epoch. Anything it
   had applied that the checkpoint leaves out is undone, and the signed
   order behind it goes back to being an unfinished promise. Where a
   validator had voted for one order and then applied a certificate for a
   different one under the same number, it stays promised to the one that
   had the certificate.

A validator that knows which checkpoint was decided but cannot work out the
state itself, because it was away or restarted, fetches the state from a
validator that has moved on. It accepts the state only if it hashes to the
state root in the checkpoint.

If fewer than two thirds of the stake are online, the close cannot finish
and the network stays paused. Payments could not become final in that
situation anyway.

### 6. Cancellation and spare keys

- Every address commits to two things: the first key and a **spare tree**,
  sixteen one-time keys under one root. The first payment reveals the root.
- An account is always in one of three conditions, which says what must
  sign its next order: its first key, if it has never signed; the key its
  last order named; or a numbered spare key, after a cancellation.
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
  checkpoint, time, state root, the number of payments, and the validators
  of the next epoch. Totals for fees and released coins join it when those
  are built. A wallet keeps the latest header and the list of validator
  sets, each checked against the one before, back to the genesis file.
- It names the statements it was built from. Payments are not listed:
  validators already hold the certificates, and the state root pins the
  result. Cancellations are not listed either. They follow, by a fixed
  rule, from the evidence in those statements, and each shows in the
  cancelled account's state.
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

A validator keeps a journal of everything it hears and says while closing
an epoch. The journal is forced to disk before anything the validator
signs leaves it. After a restart the journal is played back, which puts the
validator exactly where it was, so it cannot say something different from
what it said before.

State is kept as one snapshot and one log per epoch, so that a restart
after a checkpoint cannot replay payments the checkpoint left out.

### 10. Time and keys

- **Time.** A validator checks a new proposal's time against its own
  clock. Coins are released according to the time that has passed between
  checkpoints, not per checkpoint, so closing early or late does not change
  how many coins exist.
- **Keys.** A validator stops voting on payments when its key tree is down
  to a reserve kept for closing messages. It starts building a replacement
  once half its keys are used, names the new tree in its closing
  statements, and signs with it from the first epoch whose checkpoint lists
  it. Everything a validator signs in one step of a close is signed as one
  batch, with one key.

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
- the **spare root**. An account's first order reveals it, and validators
  check that it and the first key together make the address. Every later
  order repeats it.

An owner's signature is one of two kinds: made with the account's current
one-time key, or made with one of its spare keys together with the path
that ties that key to the spare root.

## Built on top of the close

### Staking and the validator set

- A stake order locks coins against a validator's key-tree root and
  address. An unstake order starts a waiting period of several epochs.
- A stake order carries the validator's own signature, made with the key
  tree it names, over the staking account, the address and the network.
  Without it anyone could name a working validator's key and push that
  validator out by staking more.
- The validator set for the next epoch is fixed in the checkpoint from the
  stakes recorded in it, with a minimum stake and a maximum number of
  validators. A newcomer learns the current validators and their addresses
  by following the checkpoints from the genesis file.
- Two votes for different orders under one number within one epoch, or two
  signatures from one key, cost a validator its stake at the next
  checkpoint. The proof is the validator's own signatures, carried in a
  closing statement. Its stake, including coins still waiting to be
  released, is burned: moved to an address no key can sign for, so the
  supply still adds up. It leaves the list, and a launch seat caught
  cheating loses its seat for good. The key kept for stake proofs is
  exempt, since it signs one proof per stake it backs.
- Delegating stake to someone else's validator is left for later.
- The limits, chosen on 2026-11-14, are settings in the genesis file and
  can be revisited before the launch genesis is written: a stake of at
  least 10,000 coins, at most 33 validators, and a wait of 168 epochs (one
  week) before unstaked coins can be spent. The validators of the genesis
  file are the launch seats and are exempt from the minimum.
- **The launch seats fade as real stake arrives** (chosen on 2026-11-14).
  Together they weigh what their stakes in the genesis file add up to,
  less the stake of the validators chosen, shared in the genesis file's
  proportions. Once real stake reaches that sum the seats leave the set
  for good. With seats worth 3 million coins, a newcomer's 10,000 is a
  third of a percent of the vote, and a third of the vote costs a million
  coins. The fade follows real stake rather than the calendar, so the
  vote never becomes cheap to buy while little is staked. Stake behind a
  seat's own key tree adds to that seat's weight and counts as real stake,
  so a seat's operator can carry it into the staked set.
- A validator outside an epoch's list follows the network and signs
  nothing. Its place in the list, found by its key tree, can change at
  every checkpoint.

### Fees

A payment pays a share of its amount, never less than a minimum and never
more than a maximum (chosen on 2026-11-14). A share keeps the fee about
the same in real money however the coin's value moves; the minimum stops
floods of tiny payments, and the maximum keeps a large payment from paying
far more than the work it costs. Stake and unstake orders pay the minimum.
The starting values, settings in the genesis file, are 0.1%, at least
0.000001 coin and at most 1 coin. The fee is taken from the sender when the
order is applied, into a pot no key can sign for. At each checkpoint the
pot is paid to the epoch's validators with real stake, in proportion to
it, into their staking accounts' spendable coins. Launch seats earn no
fees. What does not divide evenly, and everything in an epoch with no
staked validators, stays in the pot for the next checkpoint, so the first
people to stake collect the fees gathered before them. A validator caught
cheating earns nothing.

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
| 1a, built | Epoch number (always 0 for now) in votes and certificates. Certificates that keep the owner's signature. Signed confirmations. Receipts that carry them. A wallet that advances only on proof. | `crates/ledger`, `crates/net`, `crates/cli` |
| 1b, built | The order format, fixed once: kind, largest fee, spare tree, the new address form. Cancellation as a pure function. The wallet's key streams. | `crates/ledger`, `crates/net/src/keys.rs`, `crates/crypto/src/hash.rs` |
| 2, built | The close as pure logic with no network or disk, and simulators that run it under delays, absent validators and dishonest ones. The failures the review found are permanent tests. | `crates/ledger/src/agree.rs`, `crates/ledger/src/close.rs` |
| 3, built | The close inside the validator with a fixed validator set: the pause, messages between validators, the journal, the switch of state, the checkpoint chain, account proofs, key-tree replacement. Wallets follow checkpoints and sign with a spare key after a cancellation. | `crates/net/src/closer.rs`, `crates/net/src/node.rs`, `crates/net/src/client.rs`, `crates/cli` |
| 4, built | Staking and unstaking with a wait, a validator set chosen by stake at each checkpoint, launch seats that fade as real stake arrives, validators that join from outside the genesis file, wallets that follow the set, and penalties for cheating validators. | all crates |
| 5 | Fees. | `crates/ledger`, `crates/net` |
| 6 | Coin creation, a genesis with no coins, and a command that does the work. | all crates |

Stuck accounts are freed from stage 3 on.

## Not decided

- **A slow sunrise.** Whether the release rate starts near zero and climbs
  to full over the first 90 days.
- **The launch seats' total weight.** Test networks use 3 million coins.
- **The launch values of the fee.** A coin worth far more than today makes
  the minimum and maximum dearer; only very small and very large payments
  feel that.
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

## What building stage 2 found

The agreement was built as pure logic and run in a simulator against
thousands of message orders, with validators absent and one lying. Three
ways it could stall turned up. None allowed a wrong decision. Each left the
network unable to decide anything, and each is fixed in rule 5.

- **A vote shown to one validator.** A dishonest validator sends the vote
  that completes two thirds to a single honest validator. That validator
  acts on it and the others cannot follow, having never seen the vote. So
  proof now travels with the message that rests on it.
- **Conflicting votes.** A dishonest validator sends different votes for
  one round to different validators. Each validator counted the first it
  saw, so a vote passed on later as proof was ignored by anyone who had
  already seen a different one. Proof is now judged as a whole, apart from
  what the receiver has seen before.
- **A decision not passed on.** A dishonest validator shows genuine proof
  of a decision to one validator, which decides and takes no further part,
  leaving the rest short of two thirds. A validator that learns of a
  decision now passes its proof to everyone.

A fourth lesson was about the tests. A simulated liar that lies at random
almost never finds the sequence that exploits a missing safety rule: with
random lies, five deliberately removed rules went unnoticed. The simulated
liar now plays the known attacks, and one attack on the lock is scripted
message by message.

## What building stage 3 found

- **A wallet could pay twice.** A payment can go through without the wallet
  hearing so: enough validators record it, and too few answer in time. A
  wallet that then signs "the same payment again" is making a second one.
  The `orien` command already guarded against this. The test helper did
  not, and paid 61 times when asked for 60. The rule, now also in the
  design: before signing, a wallet checks whether the payment it signed
  earlier has gone through, and treats a matching request as already done.
- **A small key tree lost every key on restart.** Key numbers are reserved
  on disk in blocks, and a restart skips to the end of the block. The block
  was 64, which is the whole of a 64-key tree. Blocks now scale with the
  tree.
- **The tests did not notice a forgetful validator.** With one rule broken
  on purpose at a time, five breaks went unnoticed, all of the same kind: a
  validator that restarts and has forgotten part of what it heard or how it
  voted. The tests checked that a restarted validator never says two
  different things, but the other validators decide so quickly that a
  forgetful one never gets the chance. The tests now check directly that a
  validator comes back from a restart knowing exactly what it knew.
- **A validator that stops voting must say so at once.** The threads that
  talk to other validators slept through the start of a close for up to a
  second. They are now woken the moment it begins.

### Left for later, on purpose

- **A receipt for a payment settled by a checkpoint.** A payment that had
  its certificate when the epoch closed, and that the checkpoint took in,
  is final without any confirmations. The wallet sees that and says so, but
  cannot yet hand anyone a proof of it.
- **Old epochs are kept forever.** Each epoch's snapshot and log stay on
  disk. Dropping old ones needs a rule for how far back a returning
  validator may ask. Wanted soon after stage 4: with 33 validators a
  validator stores about 40 KB of votes per payment, about 1.4 GB an hour
  at ten payments a second, and a checkpoint makes all of it redundant.
- **A validator whose key tree is used up completely cannot recover by
  itself.** It needs a key to announce a replacement. Replacement starts at
  half used, so this takes a long outage at the wrong moment. Stage 4,
  where validators are named by stake orders, is where it gets a way back.
- **Proof of a validator cheating is only kept in memory,** and only for
  the epoch it happened in. A validator restarted mid-epoch forgets what
  it found, and proof found after a validator has stopped work on the
  epoch is dropped. A cheat seen only by a wallet, two votes for orders
  that never both got certificates, is not passed on.
- **A wallet that has been away a long time knows only the validators of
  its last visit.** If all of them have left since, it has nobody to ask.
  Falling back to the genesis file's validators would cover it.
- **A wallet that has been away follows every checkpoint it missed.** That
  is one small download per epoch. A shortcut for long absences is not
  built.
- **Checking a checkpoint against a lying validator's wrong state** is one
  comparison in the code and has no test that plays the lying validator.

