import type { Metadata } from 'next'
import Link from 'next/link'
import { ArrowRight, Download } from 'lucide-react'
import { Formula, PaperSection, Sub, Table } from '@/components/Paper'
import { OPEN_CLAIMS_FROM, RANDOMX_FROM, SITE_URL } from '@/lib/brand'
import { jsonLd, pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Orien technical paper',
  description:
    'The Orien technical paper: the exact rules of the Orien cryptocurrency, from its BLAKE3 hash-based one-time signatures and payment certificates to hourly checkpoints, staking, fees, RandomX mining and atomic swaps with Bitcoin.',
  path: '/paper/',
})

const SECTIONS = [
  { id: 'introduction', title: 'Introduction' },
  { id: 'building-blocks', title: 'Building blocks' },
  { id: 'accounts', title: 'Accounts and orders' },
  { id: 'finality', title: 'How a payment becomes final' },
  { id: 'validators', title: 'Validators and stake' },
  { id: 'checkpoints', title: 'Epochs and checkpoints' },
  { id: 'fees', title: 'Fees' },
  { id: 'coins', title: 'New coins' },
  { id: 'swaps', title: 'Locked payments and swaps' },
  { id: 'genesis', title: 'Genesis' },
  { id: 'network', title: 'The network' },
  { id: 'wallets', title: 'Wallets' },
  { id: 'releases', title: 'Software releases' },
  { id: 'security', title: 'Security' },
  { id: 'measured', title: 'Measured performance' },
  { id: 'limitations', title: 'Known limitations' },
  { id: 'parameters', title: 'Parameters' },
  { id: 'references', title: 'References' },
]

export default function PaperPage() {
  return (
    <article className="mx-auto max-w-[760px] print:max-w-none">
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={jsonLd({
          '@type': 'TechArticle',
          headline: 'Orien technical paper',
          description:
            'The exact rules of the Orien cryptocurrency: hash-based one-time signatures, payment certificates, hourly checkpoints, staking, fees, RandomX mining and atomic swaps with Bitcoin.',
          mainEntityOfPage: `${SITE_URL}/paper/`,
          image: `${SITE_URL}/og.png`,
          encoding: { '@type': 'MediaObject', contentUrl: `${SITE_URL}/orien-paper.pdf`, encodingFormat: 'application/pdf' },
          author: { '@id': `${SITE_URL}/#orien` },
          publisher: { '@id': `${SITE_URL}/#orien` },
        })}
      />
      <header className="pb-6 pt-10 sm:pt-14 print:pt-0">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted print:text-neutral-500">
          Technical paper · version 1.1
        </p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl print:text-3xl print:text-black">
          Orien: a quantum-proof cryptocurrency without blocks
        </h1>
        <p className="mt-6 text-lg leading-relaxed text-muted print:text-[11pt] print:text-neutral-800">
          Orien is a cryptocurrency whose coin is ORN. Each account orders its own payments, and a payment is final as
          soon as validators holding more than two thirds of the stake have vouched for it, with no blocks and no global
          queue. Validators agree on anything only once an hour, at a checkpoint. Every signature is a hash-based
          one-time signature built on BLAKE3 alone, so the system rests on a single assumption that quantum computers
          are not known to break. New coins, 100 million in all, are released on a fixed curve and earned by
          proof-of-work.
        </p>
        <p className="mt-4 text-sm leading-relaxed text-muted print:text-[9pt] print:text-neutral-600">
          This paper states the rules exactly as version 1.1 of the <strong className="text-ink print:text-black">orien</strong> program
          implements them. Where the two could ever differ, the program’s source code is the reference.
        </p>
        <a
          href="/orien-paper.pdf"
          download
          className="mt-6 inline-flex h-11 items-center gap-2 rounded-xl border border-line bg-surface px-5 text-sm font-semibold text-ink transition hover:border-pearl print:hidden"
        >
          <Download className="h-4 w-4" aria-hidden /> Download the PDF
        </a>
      </header>

      <nav
        aria-label="Contents"
        className="mb-4 rounded-2xl border border-line bg-surface p-5 print:border-neutral-300 print:bg-white"
      >
        <p className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted print:text-neutral-500">Contents</p>
        <ol className="grid gap-x-6 gap-y-1.5 text-sm sm:grid-cols-2">
          {SECTIONS.map(({ id, title }, i) => (
            <li key={id}>
              <a href={`#${id}`} className="text-muted transition hover:text-ink print:text-neutral-800">
                <span className="mr-2 tabular-nums">{i + 1}.</span>
                {title}
              </a>
            </li>
          ))}
        </ol>
      </nav>

      <PaperSection id="introduction" number={1} title="Introduction">
        <p>
          Most cryptocurrencies settle payments by agreeing on a single, ever-growing sequence of blocks. Every payment
          in the world waits in the same queue, and finality is a matter of waiting for enough blocks on top of it. Orien
          starts from a different observation: a payment only needs to be ordered relative to other payments from the
          same account. Two payments from different people never conflict, so they never need to wait for each other.
        </p>
        <p>
          Each Orien account therefore numbers its own orders 0, 1, 2 and so on. Validators check each order on arrival
          and sign it if it is the account’s next number and the account can afford it. Once validators holding more than
          two thirds of the stake have signed, and then confirmed that they hold that certificate, the payment is final.
          Validators never sign two different orders under the same number of the same account, so no two conflicting
          payments can both gather a quorum.
        </p>
        <p>
          Some decisions do need agreement among all validators: which payments an epoch contained, who validates next,
          who cheated, how fees and new coins are shared. Orien makes them once an hour, at a checkpoint, with a
          classical agreement protocol. Between checkpoints, payments flow without any agreement round at all.
        </p>
        <p>The design follows four goals:</p>
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>Quantum-proof by construction.</strong> Every signature is a hash-based one-time signature. The
            whole system assumes only that BLAKE3 behaves as a secure hash function.
          </li>
          <li>
            <strong>Fast, independent finality.</strong> A payment is final after two short round trips to the
            validators, and is never delayed by anyone else’s payments.
          </li>
          <li>
            <strong>Checkable by anyone.</strong> A payment’s receipt proves it final to anyone holding the network’s
            genesis file and its checkpoints, offline. Balances are proven by checkpoints that wallets check
            themselves.
          </li>
          <li>
            <strong>A fair start.</strong> The network starts with no coins. Every coin is released on a fixed schedule
            to whoever does proof-of-work, beginning at a Bitcoin block named in advance.
          </li>
        </ul>
        <p>
          The building blocks that make this work are known: per-account ordering with quorum certificates follows the
          line of work on consensusless payment systems such as FastPay [1], the hourly agreement is Tendermint [2], and
          the signatures are the Winternitz one-time signatures and Merkle key trees that underlie the SLH-DSA standard
          [3, 4]. Orien combines them into one small, complete system.
        </p>
        <p>
          Throughout, amounts are counted in <strong>units</strong>: one ORN is 10<sup>9</sup> units, so balances have
          nine decimal places. Structures are encoded with Borsh [5], and integers are little-endian.
        </p>
      </PaperSection>

      <PaperSection id="building-blocks" number={2} title="Building blocks">
        <Sub title="2.1 One hash function">
          <p>
            Orien uses a single cryptographic primitive, the BLAKE3 hash function [6], with 32-byte outputs. Every use is
            separated by a one-byte domain tag, so that a value hashed for one purpose can never be mistaken for a value
            hashed for another:
          </p>
          <Formula>{`hash(tag, part₁, …, partₙ) = BLAKE3(tag ‖ part₁ ‖ … ‖ partₙ)`}</Formula>
          <p>
            Secret values are derived from seeds with BLAKE3 in keyed mode, as a pseudorandom function over a one-byte
            purpose and a 64-bit index:
          </p>
          <Formula>{`prf(key, purpose, i) = BLAKE3_keyed(key, purpose ‖ u64_le(i))`}</Formula>
          <p>The tags are listed in the appendix.</p>
        </Sub>

        <Sub title="2.2 One-time signatures">
          <p>
            A one-time key signs exactly one message. Orien uses the Winternitz one-time signature (WOTS) with base{' '}
            <em>w</em> = 16 and 24-byte chain values, the same shape as the WOTS layer of SLH-DSA-192 [3]. A key is a
            32-byte seed. From it the PRF derives a 24-byte public seed and 51 chain starts. Each chain is walked 15
            steps:
          </p>
          <Formula>{`step(pub_seed, chain, position, v) = BLAKE3(0x01 ‖ pub_seed ‖ chain ‖ position ‖ v)[0..24]
endᵢ = 15 steps from startᵢ
commitment = hash(0x02, pub_seed ‖ end₀ ‖ … ‖ end₅₀)          (32 bytes)`}</Formula>
          <p>
            The commitment is the public key. To sign a message, its digest D = hash(0x03, pub_seed ‖ message) is computed, and
            the first 24 bytes of D are cut into 48 base-16 digits d₀…d₄₇ (high nibble first), followed by three checksum digits of c = Σ(15 − dᵢ). The
            signature reveals the public seed and, for each of the 51 digits, the chain value dᵢ steps from its start.
            A verifier walks each revealed value the rest of the way to step 15 and recomputes the commitment.
          </p>
          <p>
            A signature is 24 × 52 = <strong>1,248 bytes</strong>. The checksum means that forging a signature on a
            second message would require walking some chain backwards, which is inverting BLAKE3. Signing two different
            messages with one key reveals enough to forge; the rules in sections 3 and 6 make sure no honest key ever
            does, and turn any key that does into evidence.
          </p>
        </Sub>

        <Sub title="2.3 Key trees">
          <p>
            Validators sign many messages, so they use a key tree: a Merkle tree [7] whose 2<sup>h</sup> leaves are the
            commitments of one-time keys, with leaf <em>j</em> derived as prf(tree_seed, 0x04, j). Inner nodes are
            hash(0x10, left ‖ right), and the root is the public key. A tree signature names its leaf, carries that
            leaf’s one-time signature, and the h sibling hashes from leaf to root:
          </p>
          <Formula>{`TreeSignature { leaf: u32, ots: Signature, path: [Hash32; h] }     1,256 + 32·h bytes`}</Formula>
          <p>
            Heights are at most 24. A validator key has height 16 by default (65,536 one-time keys), and signs whole
            batches at a time (section 4), so one leaf covers many payments.
          </p>
        </Sub>

        <Sub title="2.4 Merkle trees of items">
          <p>
            Lists that are signed or proven together (a batch of votes, the set of all accounts) form complete binary
            Merkle trees, padded to a power of two. Two kinds keep them apart: batches use tags 0x20, 0x21 and 0x22 for
            leaves, nodes and padding, and the state uses 0x40, 0x41 and 0x42. A path proves one item’s place in a tree
            of depth at most 32.
          </p>
        </Sub>
      </PaperSection>

      <PaperSection id="accounts" number={3} title="Accounts and orders">
        <Sub title="3.1 Addresses">
          <p>
            An account is controlled by a chain of one-time keys, one per order, plus 16 spare keys held in a key tree of
            height 4. Its address commits to the first key and to the spare tree:
          </p>
          <Formula>{`address = hash(0x35, first_commitment ‖ spare_root)`}</Formula>
          <p>
            An address is 32 bytes, written as 64 hexadecimal characters. No public key is revealed until the account’s
            first order, and each order names the key that will sign the next one, so a key is exposed only for the
            short time between signing and finality.
          </p>
        </Sub>

        <Sub title="3.2 Orders">
          <p>Every change to an account is an order, signed by the account:</p>
          <Formula>{`Order {
  kind:     Payment | Stake { validator, proof } | Unstake | Lock { hash, until } | Claim { from, seq, secret }
  from:     AccountId          the paying account
  seq:      u64                its order number: 0, 1, 2, …
  to:       AccountId          recipient (the account itself for stake and unstake)
  amount:   u64                in units
  max_fee:  u64                the largest fee the owner accepts
  next_key: Hash32             commitment of the key that will sign order seq + 1
  spare:    Hash32             root of the account's spare keys
}
digest = hash(0x30, network_id ‖ borsh(order))`}</Formula>
          <p>
            The digest covers the network’s identity (section 10), so an order is valid on one network only. A payment
            order is 153 bytes and its signature 1,249 bytes.
          </p>
          <p>
            An account’s state records its balance, its next order number, and which key must sign next: its first key
            (until it has signed), the key named by its last order’s <code>next_key</code>, or a numbered spare key after
            a cancellation (section 4.5). An order is accepted only under the account’s current number and only with
            exactly the key the account expects.
          </p>
        </Sub>

        <Sub title="3.3 What a validator checks">
          <p>
            Before voting for an order, a validator first checks that its signature is well formed and that a stake
            order carries a valid proof from its validator (section 5.1). Then it checks, in this order, and refuses on
            the first failure:
          </p>
          <ol className="list-decimal space-y-1 pl-5">
            <li>the amount is not zero, and the paying account exists;</li>
            <li>the order number is the account’s current one, not an earlier or later one;</li>
            <li>it is signed with the key the account expects;</li>
            <li>this validator has not already voted for a different order under that number;</li>
            <li>the recipient is not one of the network’s own accounts (section 3.4);</li>
            <li>a stake or unstake names the account itself, and a stake names a usable validator;</li>
            <li>
              <code>max_fee</code> covers the fee the network charges (section 7);
            </li>
            <li>
              the balance covers the amount plus the fee (for an unstake: the fee, and enough coins staked to release).
            </li>
          </ol>
          <p>
            A validator records which order it voted for under each (account, number) <em>before</em> its vote leaves
            the machine, writing it durably to disk, so that not even a crash can make it vote twice.
          </p>
        </Sub>

        <Sub title="3.4 Reserved accounts">
          <p>Four addresses belong to no key and are used by the system itself:</p>
          <Table
            head={['Account', 'Address', 'Holds']}
            rows={[
              ['Forfeited', '00…00', 'stake taken from validators caught cheating; never spendable'],
              ['Fees', '01…01', 'fees collected during the current epoch, paid out at the checkpoint'],
              ['Mint', '02…02', 'minus the number of coins released so far, so the total of all accounts stays zero'],
              ['Rewards', '03…03', 'released coins waiting to be paid to miners'],
            ]}
          />
          <p>
            Payments to any of them are refused, so that no coin reaches them by mistake.
          </p>
        </Sub>
      </PaperSection>

      <PaperSection id="finality" number={4} title="How a payment becomes final">
        <Sub title="4.1 Votes, signed in batches">
          <p>
            A validator signs its votes in batches. It gathers the items to sign, builds a Merkle tree over them, and
            signs the root once with one leaf of its key tree:
          </p>
          <Formula>{`batch_message = hash(0x31, network_id ‖ u64_le(epoch) ‖ batch_root)
Vote { validator: u16, batch_path: Path, signature: TreeSignature }`}</Formula>
          <p>
            A vote proves its item through the batch path, so one tree signature serves every payment in the batch. The
            busier the network, the more payments each one-time key covers.
          </p>
        </Sub>

        <Sub title="4.2 Certificates">
          <p>
            The paying wallet sends its signed order to every validator and collects their votes. Votes from validators
            holding more than two thirds of the epoch’s total stake form a certificate:
          </p>
          <Formula>{`Certificate { epoch, order, signature, votes }        valid when 3 · stake(votes) > 2 · total_stake`}</Formula>
          <p>
            Votes are listed by increasing validator number, and each is checked against that validator’s key and the
            certificate’s epoch.
          </p>
        </Sub>

        <Sub title="4.3 Confirmations and finality">
          <p>
            The wallet then hands the certificate to the validators. Each applies the order to its copy of the ledger
            and signs a confirmation of the item hash(0x34, order_digest), again in batches. A certificate together with
            confirmations from more than two thirds of the stake is a receipt, and the payment is <strong>final</strong>
            :
          </p>
          <Formula>{`Receipt { certificate, confirmations }        valid when both carry more than two thirds of the stake`}</Formula>
          <p>
            Anyone with the genesis file can check a receipt without contacting the network: it is checked against
            the validators of its own epoch, which the genesis file names for the first epoch and the checkpoints name
            after that. The second step matters at checkpoints: a payment with a receipt is known to more than two thirds
            of the validators, so it cannot be left out of the epoch’s record (section 6).
          </p>
        </Sub>

        <Sub title="4.4 Applying a certified order">
          <p>
            A validator applies a certificate even if it did not vote for it, as long as the order is the account’s next
            one. It moves the amount, charges the fee into the fees account, records the next key, and increases the
            account’s order number. Balances are signed numbers internally, because certificates for payments into an
            account can arrive before or after payments out of it.
          </p>
        </Sub>

        <Sub title="4.5 Two orders under one number">
          <p>
            An honest wallet signs one order per number. If the owner, or a thief with a copy of the wallet, signs two
            different orders under the same number, honest validators vote for whichever they see first. Neither may then
            reach a quorum, and the account is stuck at that number. The two signed orders are themselves the evidence:
            any validator includes them in its statement at the checkpoint, and the checkpoint cancels that number
            unless one of the two orders already became final. Nothing moves, and from then on the account must sign
            with its next spare key. After all 16 spare keys are used, the account stays locked.
          </p>
          <p>
            Two signatures from one one-time key reveal enough of its chains that a third order can sometimes be forged
            from them, and such an order could gather votes before the checkpoint. Signing twice is therefore never
            safe, and an honest wallet never does it. Spare keys protect an account against the forgery of a burned
            key; they do not protect it from a thief who holds a copy of the whole wallet, since that copy holds the
            spare keys too.
          </p>
        </Sub>

        <Sub title="4.6 Why conflicting payments cannot both be final">
          <p>
            Two certificates for different orders under the same account and number would each need votes from more
            than two thirds of the stake. Any two such sets overlap in more than one third of the stake. If validators
            holding at most one third misbehave, at least one honest validator lies in the overlap, and it never votes
            twice. So at most one of the two orders can ever be certified, and no coin can be spent twice.
          </p>
        </Sub>
      </PaperSection>

      <PaperSection id="validators" number={5} title="Validators and stake">
        <Sub title="5.1 Staking">
          <p>
            Anyone holding ORN can lock coins behind a validator with a stake order. The order names the validator’s key
            (its root and height) and the address where it can be reached, and carries a proof that the validator agrees,
            signed with leaf 0 of the validator’s key tree, which is set aside for this purpose:
          </p>
          <Formula>{`stake_proof = sign(leaf 0, hash(0x45, network_id ‖ account ‖ address))`}</Formula>
          <p>
            An unstake order releases staked coins. They stay locked, and can still be taken for cheating, for 168
            epochs (about a week, at one epoch an hour), then return to the account’s spendable balance.
          </p>
        </Sub>

        <Sub title="5.2 Choosing the validators">
          <p>
            At each checkpoint the next epoch’s validators are chosen deterministically from the state. Accounts with at
            least <strong>10,000 ORN</strong> staked behind a usable validator are sorted by stake, largest first, ties
            broken by account address, and the first <strong>33</strong> distinct validator keys are taken. Each votes
            with a weight equal to its stake. Validators proven to have cheated are barred for good, and so is any key they used.
          </p>
        </Sub>

        <Sub title="5.3 Launch seats">
          <p>
            A network that starts with no coins has no stake. So it starts with launch seats: validators named in the
            genesis file, which together carry a fixed weight (3 million ORN of weight in the launch configuration),
            shared equally. Seats give way as real stake arrives. With <em>real</em> the total stake of chosen stakers
            and <em>seats</em> the seats’ combined starting weight, each seat’s weight at a checkpoint is:
          </p>
          <Formula>{`weight(seat) = max( ⌊(seats − real) · start_weight(seat) / seats⌋, 1 ) + backing(seat)`}</Formula>
          <p>
            Stake placed behind a seat’s own key counts as that seat’s backing, not as a separate validator. Once real
            stake reaches the seats’ starting weight, the seats leave for good and only stakers remain. Seats never
            earn fees.
          </p>
        </Sub>

        <Sub title="5.4 Penalties">
          <p>Two kinds of cheating are provable from signatures alone:</p>
          <ul className="list-disc space-y-2 pl-5">
            <li>
              <strong>Two votes</strong>: one validator voting for two different orders under the same account and
              number.
            </li>
            <li>
              <strong>Key reuse</strong>: one leaf of a validator’s key tree signing two different messages (other than
              the stake proof leaf).
            </li>
          </ul>
          <p>
            Proof travels in validators’ statements. At the checkpoint, a convicted staked validator loses everything its
            account holds in stake (staked, unstaking and waiting to be released), moved to the forfeited account; a
            convicted seat loses its seat. Either way its key is barred from the validators for good: the ledger state
            keeps a list of barred keys, and the state root covers it, so the list survives every checkpoint and every
            validator that catches up by jumping.
          </p>
        </Sub>
      </PaperSection>

      <PaperSection id="checkpoints" number={6} title="Epochs and checkpoints">
        <Sub title="6.1 Epochs">
          <p>
            Time is divided into epochs of one hour. Each epoch has a fixed set of validators, and every vote and
            certificate names its epoch. A validator starts closing an epoch when its hour is up, after 50,000
            payments, or as soon as it sees validators holding more than a third of the stake closing it.
          </p>
        </Sub>

        <Sub title="6.2 Statements">
          <p>
            To close an epoch, each validator publishes a signed statement of what it saw: a running hash, or pin, of
            every payment it applied, in the order it applied them; evidence of orders signed twice; proof of cheating
            validators; the proof-of-work shares it received; and optionally a new key for the next epoch.
          </p>
          <Formula>{`pin₀ = hash(0x37, u64_le(epoch)),   pinₖ₊₁ = hash(0x37, pinₖ ‖ order_digest)
Statement { epoch, validator, log: Pin, evidence, next_key, cheats, shares }`}</Formula>
        </Sub>

        <Sub title="6.3 The checkpoint">
          <p>
            A checkpoint merges statements from validators holding more than two thirds of the stake. The epoch’s
            payments must include every payment in the logs those statements pin. Because a final payment was confirmed by more than
            two thirds of the stake, and any two such sets overlap in an honest validator, every final payment is in
            some merged log.
          </p>
          <Formula>{`Checkpoint {
  epoch, previous,           this epoch, and the digest of the previous checkpoint
  time,                      the proposer's clock
  state,                     Merkle root of every account, and of the barred keys
  payments, merged,          how many payments, and which statements
  validators,                the next epoch's validators and their weights
  released, target           new coins released, and the next proof-of-work target
}`}</Formula>
          <p>Closing applies, in this order:</p>
          <ol className="list-decimal space-y-1 pl-5">
            <li>every payment, sorted by account and order number, each of which must be certified;</li>
            <li>a check that no account other than the mint is overdrawn;</li>
            <li>cancellations for orders signed twice (section 4.5);</li>
            <li>locked payments whose time is up, returned to their senders (section 9);</li>
            <li>forfeits for convicted validators (section 5.4);</li>
            <li>payout of the epoch’s fees (section 7);</li>
            <li>release of new coins and their payout to miners (section 8);</li>
            <li>new keys announced by staked validators;</li>
            <li>unstaked coins that have waited long enough, returned to their owners.</li>
          </ol>
        </Sub>

        <Sub title="6.4 Agreement">
          <p>
            Validators agree on the checkpoint with Tendermint [2], rule for rule. In each round a proposer, drawn by
            weight from a seed taken from the previous checkpoint, proposes a checkpoint together with the statements it
            merges. Validators check it by recomputing it from the statements, prevote, then precommit; a checkpoint
            precommitted by more than two thirds of the weight is decided, and the next epoch begins. The rule of locking
            on a precommitted value keeps two different checkpoints from ever being decided for one epoch, as long as no
            more than a third of the weight is faulty.
          </p>
        </Sub>
      </PaperSection>

      <PaperSection id="fees" number={7} title="Fees">
        <p>Every payment pays a fee of 0.1% of its amount, between a floor and a ceiling:</p>
        <Formula>{`fee(payment) = clamp( ⌊amount · rate / 1,000,000⌋, min, max )
rate = 1,000 (0.1%)    min = 1,000 units (0.000001 ORN)    max = 10⁹ units (1 ORN)
fee(stake) = fee(unstake) = min`}</Formula>
        <p>
          A share of the amount keeps the fee about the same in real terms whatever a coin is worth. The floor stops
          floods of tiny payments, and the ceiling keeps large payments from paying far more than they cost the network.
          The fee is paid on top of the amount, and an order states the largest fee its owner accepts.
        </p>
        <p>
          Fees collect in the fees account during the epoch. At the checkpoint they are shared among that epoch’s staked
          validators that were not caught cheating, in proportion to stake: each receives ⌊pot · stake / total⌋, and the
          remainder waits for the next checkpoint.
        </p>
      </PaperSection>

      <PaperSection id="coins" number={8} title="New coins">
        <Sub title="8.1 The release curve">
          <p>
            ORN are created only by release, on a fixed curve that approaches 100 million coins and halves what remains
            every four years. With <em>t</em> the seconds since the network’s start time:
          </p>
          <Formula>{`released(t) = CAP · (1 − 2^(−t / HALF_LIFE))
CAP = 100,000,000 ORN = 10¹⁷ units       HALF_LIFE = 126,230,400 s  (4 × 365.25 days)`}</Formula>
          <p>
            Exact arithmetic matters, since every validator must compute the same number. The curve is evaluated in
            64-bit fixed point: PER_SECOND = 18,446,743,972,416,134,102, which is 2<sup>−1/HALF_LIFE</sup> × 2<sup>64</sup>{' '}
            rounded to nearest, is raised to the power (t mod HALF_LIFE) by square-and-multiply with every product rounded
            to nearest, then halved once per whole half-life:
          </p>
          <Formula>{`h = ⌊t / HALF_LIFE⌋,   u = PER_SECOND^(t mod HALF_LIFE) >> h        (u = 0 when h ≥ 64)
released(t) = CAP − ⌊CAP · u / 2⁶⁴⌋`}</Formula>
          <p>
            This releases about 1,977 ORN in the first hour, 47,432 in the first day, 50 million in the first four years
            and 75 million in the first eight. At each checkpoint the coins released since the previous one move from the
            mint to the rewards account. The amount depends on elapsed time, not on the number of epochs.
          </p>
        </Sub>

        <Sub title="8.2 Proof-of-work shares">
          <p>
            Miners earn released coins with shares: numbers that, hashed with the previous checkpoint and the miner’s
            account, fall below the epoch’s target. The hash is RandomX [9], the proof-of-work Monero uses, keyed with
            the epoch’s seed:
          </p>
          <Formula>{`work = u64_le( RandomX(key = seed, input = seed ‖ account ‖ u64_le(nonce))[0..8] )
a share counts when work < target           seed = digest of the previous checkpoint`}</Formula>
          <p>
            Each RandomX hash runs a random program over a 2 GiB dataset built from the key: work an ordinary processor
            does well, and at which graphics cards and custom chips gain little, so mining stays open to anyone with a
            computer. Miners rebuild the dataset for each epoch’s seed; validators check shares in RandomX’s light mode,
            with a 256 MiB cache, and each share once. Before epoch {RANDOMX_FROM.toLocaleString('en')} the work was
            BLAKE3: <code>u64_le( hash(0x46, seed ‖ account ‖ u64_le(nonce))[0..8] )</code>.
          </p>
          <p>
            Because the seed is the previous checkpoint, no share can be computed before that checkpoint exists. Miners
            send shares to validators, which list them in their statements. At the checkpoint, the shares from the merged
            statements are deduplicated and counted per account, and the rewards account is shared out in proportion:
            each account receives ⌊pot · its_shares / all_shares⌋, and the remainder waits.
          </p>
          <p>
            Every counted share is listed in the statements that close the epoch, so each validator counts at most 40
            times the shares wanted per epoch, 40,000, and refuses the rest until the next checkpoint. With many
            validators the limit is lower, so that the proposal carrying all their statements stays under about 12 MB:
            at most 300,000 shares divided by the number of validators. The target normally keeps shares far below this
            limit; it is reached only for a few epochs after mining power rises sharply.
          </p>
          <p>
            Work only creates coins. It never decides which payments happen or in what order, so a miner with a great
            deal of computing power can earn more coins but cannot rewrite anyone’s payments.
          </p>
        </Sub>

        <Sub title="8.3 The target">
          <p>
            The target aims for 1,000 shares per epoch, so that pay is spread finely however much work is done. After
            each epoch with <em>n</em> shares under target <em>T</em>:
          </p>
          <Formula>{`T′ = 4 · T                                   if n = 0
T′ = clamp( T · 1,000 / n,  ⌊T / 4⌋,  4 · T )  otherwise`}</Formula>
          <p>
            The first target was 2<sup>40</sup> − 1: about one share for every 16.8 million BLAKE3 hashes. At epoch{' '}
            {RANDOMX_FROM.toLocaleString('en')}, the first judged by RandomX, it started again at ⌊(2<sup>64</sup> − 1) /
            400,000⌋: about one share for every 400,000 RandomX hashes. The target moves by at most a factor of four per
            epoch, which limits how far a sudden burst of work can push it.
          </p>
        </Sub>
      </PaperSection>

      <PaperSection id="swaps" number={9} title="Locked payments and swaps">
        <Sub title="9.1 Locked payments">
          <p>
            A <strong>Lock</strong> order moves an amount out of the sender’s balance into an escrow for a payee,
            claimable with the secret whose SHA-256 is <code>hash</code> until epoch <code>until</code> begins. The
            deadline must be 2 to 720 epochs after the epoch the order is voted in, and the amount must exceed the
            minimum fee. A Lock pays the same fee as a payment. Escrows are named by their sender and the Lock order’s
            number.
          </p>
          <p>
            A <strong>Claim</strong> order names the escrow, its payee as <code>to</code>, and reveals the secret.
            Validators vote for it only if the escrow is waiting for that payee and amount, the epoch is before the
            deadline, and the secret’s SHA-256 is the escrow’s hash. Any account may sign it, a payee that holds nothing
            yet included: its fee, the minimum, comes out of the escrow, and applying it pays the payee, whoever claimed,
            and records the secret, which stays readable in the state for 168 epochs. So whoever knows the secret can
            finish a payee’s claim for them, as a swap’s seller does for the buyer. Until epoch {OPEN_CLAIMS_FROM}{' '}
            only the payee could sign a claim.
          </p>
          <p>
            There is no refund order. At the checkpoint that closes epoch <code>until − 1</code>, after the epoch’s
            payments and claims have been applied, every escrow still waiting goes back to its sender. A claim and a
            refund can therefore never both happen, and the deadline is exact. Two claims of one escrow, signed by
            different accounts at the same moment, can both be certified: the first applied pays, and the second, finding
            the secret already recorded, pays nothing and takes no fee, only using its order number. Every validator ends
            with the same state whichever it applies first. A validator that learns of a claim before its lock pays the
            payee and creates no escrow when the lock arrives, and ends with the same state as every other validator.
          </p>
          <Formula>{`Escrow { to, amount, hash, until }        keyed by (sender, order number)
escrows and revealed secrets are leaves of the state root, after the accounts (tags 0x4B, 0x4C)`}</Formula>
        </Sub>

        <Sub title="9.2 Why SHA-256">
          <p>
            Bitcoin’s scripts and Ethereum’s contracts can check SHA-256 but not BLAKE3. For one secret to unlock both
            sides of a swap, the lock uses SHA-256. It is the only place Orien uses anything but BLAKE3, and it
            protects only locked payments: a weakness in SHA-256 could let someone claim a locked payment early, but
            could not touch any other part of the network.
          </p>
        </Sub>

        <Sub title="9.3 Swapping ORN for Bitcoin">
          <p>
            Two people who don’t trust each other swap ORN for Bitcoin with one secret, which the seller of ORN makes:
          </p>
          <ol className="list-decimal space-y-1 pl-5">
            <li>The seller locks the ORN for the buyer on Orien for 12 epochs.</li>
            <li>
              The buyer checks that lock on Orien, and that at least 10 epochs of it remain, then pays the Bitcoin into
              a P2WSH output the seller can spend with the secret, or the buyer after a block height 36 blocks ahead.
            </li>
            <li>
              The seller spends it once it is 3 blocks deep, while at least 6 blocks remain before the buyer could. The
              spending transaction reveals the secret.
            </li>
            <li>
              The seller’s wallet, once its Bitcoin is settled, claims the ORN for the buyer with the secret it made, so
              they arrive even if the buyer’s wallet stays closed. The buyer’s wallet, whenever it runs, also reads the
              secret from Bitcoin and claims them; whichever claim comes first pays.
            </li>
          </ol>
          <Formula>{`OP_IF
    OP_SIZE 32 OP_EQUALVERIFY OP_SHA256 <hash> OP_EQUALVERIFY <seller key> OP_CHECKSIG
OP_ELSE
    <height> OP_CHECKLOCKTIMEVERIFY OP_DROP <buyer key> OP_CHECKSIG
OP_ENDIF`}</Formula>
          <p>
            If the buyer never pays, the seller’s ORN come back at their deadline. If the seller never takes the
            Bitcoin, the buyer takes it back from the block height. The Bitcoin lock ends at least 4 hours before
            the ORN lock, so once the secret appears the buyer always has time to claim. The keys on the Bitcoin side
            are one-time keys made for the swap. The same Orien lock works against an equivalent contract on Ethereum
            for ETH and its tokens.
          </p>
          <p>
            A wallet trusts neither the other side, nor the Bitcoin explorers it reads, nor any validator’s word.
            The buyer pays only for ORN proven locked: by the Lock order’s certificate with confirmations from more
            than two thirds of the stake, or by a Merkle path from the escrow list to the last checkpoint’s state
            root. It counts the ORN claimed only on the same kind of proof. Every Bitcoin payment a wallet acts on is
            checked against the bytes of its own transaction, which hash to its id, and every spend against its
            signature. The seller reveals the secret only for payments most of the explorers it reads (two of
            three) report at least 3 blocks deep, claims only as many of them as the price needs, largest first, and only at the network’s fee rate
            with room to triple it, so tiny payments into the lock cannot make the claim too dear to be mined. The
            buyer, who must not miss a payment, reads every explorer that answers and remembers every payment it
            has seen. Claims and refunds signal replace-by-fee and are sent again until mined, at the highest fee
            rate any explorer reports, raised by a quarter every 3 blocks they stay unmined. The buyer takes each
            payment back on its own once it is mined, so a stranger’s payment into the lock cannot hold up the
            refund. It claims the ORN whenever the secret appears and keeps its refund going until they are claimed;
            from 6 hours before the ORN deadline its refund’s fee doubles every block, up to nine tenths of the coins.
            Otherwise a fee above a tenth of the coins is refused. The seller saves the secret, and the order number
            its lock will have, before locking anything, and locks under no other number, so the ORN are never locked
            twice.
          </p>
        </Sub>
      </PaperSection>

      <PaperSection id="genesis" number={10} title="Genesis">
        <p>
          A network is defined by its genesis file: its name, its launch validators (each with a key root, height,
          weight and address), any starting balances, and every rule this paper describes as a number: epoch length,
          payments per epoch, start time, minimum stake, number of validators, unstaking time, the fee rule, the cap, the
          shares wanted per epoch and the first target. The network’s identity is a hash of all of them:
        </p>
        <Formula>{`network_id = hash(0x32, name, validators (root, height, weight), allocations, every rule, bitcoin block)`}</Formula>
        <p>
          Every order, vote, statement and stake proof is bound to this identity, and checkpoints are chained from it, so
          two networks with different rules can never accept each other’s signatures. Validator names and addresses are
          not part of it, so a validator can move without changing the network.
        </p>
        <p>
          The launch configuration has no starting balances at all: every coin is earned. It names a Bitcoin block, by
          height and hash, that will be mined after the genesis file is published. The block’s hash is part of the
          network’s identity, and the identity is the seed of the first epoch’s mining, so nobody can mine before that
          block exists.
        </p>
      </PaperSection>

      <PaperSection id="network" number={11} title="The network">
        <Sub title="11.1 Messages">
          <p>
            Wallets, miners and validators talk over TCP. Each message is a 4-byte length followed by a Borsh-encoded
            request or response, at most 16 MiB. A connection carries many request and response pairs in turn. Every
            request can safely be sent twice, so a client that loses a connection simply asks again.
          </p>
        </Sub>

        <Sub title="11.2 Between validators">
          <p>
            Validators find each other through the addresses in the genesis file and in each checkpoint’s list of
            validators. Each validator keeps two links to every other one: it pushes its own closing messages and the
            proof-of-work shares it receives, and once a second (every 40 ms while an epoch is closing) it pulls the
            certificates and closing messages it has not yet seen. A validator that missed payments, or a whole epoch,
            catches up from its peers this way.
          </p>
          <p>
            Connections are not encrypted. They do not need to be for safety: every vote, certificate, statement and
            checkpoint is signed and checked on arrival, so a message cannot be forged or altered in transit, only read
            or dropped. To stay open to the internet, a validator accepts at most 1,024 connections in all and 64 from
            any one address, closes a connection that stays silent for 30 seconds, and closes one that sends anything it
            cannot decode. Other validators of the current epoch, when listed by IP address, are exempt from these limits.
          </p>
        </Sub>

        <Sub title="11.3 Durability">
          <p>
            A validator writes each promise to disk before it is kept. The signed order behind every vote is written and
            synced before the vote leaves; a certificate is synced before its confirmation leaves; each one-time key
            number is reserved on disk before it signs; and every closing message is journaled before it is sent. After
            a crash, a validator replays its records and cannot contradict anything it said before.
          </p>
        </Sub>

        <Sub title="11.4 Storage that stays small">
          <p>
            A validator keeps the ledger state of the last three epochs and the certificates of the current one. Older
            certificates, closing journals and states are deleted at each checkpoint, so the storage a validator needs
            depends on the number of accounts, not on the network’s age. Checkpoints themselves, about 40 KB an hour with
            33 validators, are kept. A validator that falls more than an epoch behind catches up in one jump: it fetches
            the checkpoints it missed, checks each one against the validators of its epoch, then fetches the latest
            state and checks it against the last checkpoint’s state root.
          </p>
        </Sub>

        <Sub title="11.5 Validators that join">
          <p>
            A validator outside the genesis file runs in watching mode until a checkpoint chooses it. It follows decided
            checkpoints and state like any other node, but neither votes nor closes epochs. When a checkpoint lists its
            key, it takes its place at the start of the next epoch.
          </p>
        </Sub>
      </PaperSection>

      <PaperSection id="wallets" number={12} title="Wallets">
        <Sub title="12.1 Trusting only what can be checked">
          <p>
            A wallet trusts no single validator. It starts from the genesis file and follows checkpoints, accepting each
            only if it continues the previous one and carries precommits from more than two thirds of that epoch’s
            weight. It remembers how far it got, so it does not start from genesis again. A balance is proven by a
            Merkle path from the account to the last checkpoint’s state root. For what happened since, a wallet counts
            only what validators holding more than two thirds of the stake all report, and a validator claiming the
            account has made more payments than the checkpoint shows must produce the certificate and confirmations for
            them. Miners take the seed and target for their work from checked checkpoints in the same way.
          </p>
        </Sub>

        <Sub title="12.2 Keys">
          <p>
            A wallet holds one 32-byte account seed. One-time key <em>n</em>, which signs the account’s order{' '}
            <em>n</em> and nothing else, is derived as prf(seed, 0x03, n), and the 16 spare keys come from
            prf(seed, 0x05, 0). Before a signed order leaves the wallet, the wallet writes it to its own file and syncs
            it. From then on it will only ever send that same order under that number. It also records what each of
            its keys has signed, and keeps that record even after letting go of an order a checkpoint cancelled, so no
            key of the wallet ever signs two different orders, whatever it is asked and even across crashes. If another copy of the same wallet signed an order, any copy
            can find it at the validators and finish it.
          </p>
        </Sub>

        <Sub title="12.3 Recovery phrase">
          <p>
            A wallet is made from a 256-bit master secret, written down as 24 words from the standard list of 2,048
            English words that most cryptocurrency wallets use. The 24 words carry 264 bits: the secret, and an 8-bit
            check taken from hash(0x48, master) rather than from SHA-256, so that an Orien phrase is almost always
            refused by wallets for other cryptocurrencies, and theirs by Orien. A word can be typed by its first four letters.
          </p>
          <p>
            The account’s seed is hash(0x49, master ‖ u32_le(1)). Restoring the phrase on another computer gives the
            same wallet there, with the same address and coins. Before each order, a wallet asks the validators for the
            account’s next number, so computers sending one after another never collide; only two copies sending in
            the same moment could sign two orders under one number (section 4.5). Account numbers other than 1, with
            seed hash(0x49, master ‖ u32_le(n)), give separate accounts with their own addresses, for anyone who
            wants coins kept apart.
          </p>
        </Sub>

        <Sub title="12.4 Passwords">
          <p>
            A wallet file can be sealed with a password. The password is stretched with Argon2id [8] (64 MiB of memory,
            3 passes), and the secret is encrypted with XChaCha20-Poly1305, with the account’s address as associated
            data so that a sealed secret cannot be moved into another wallet file. The address stays readable, so a
            balance can be checked without the password.
          </p>
        </Sub>

        <Sub title="12.5 Unlocking for a while">
          <p>
            Its owner can unlock a sealed wallet for a set time and up to a set amount, fees included, so that programs
            they trust, such as a script, can send without ever learning the password. A small helper process
            holds the opened wallet in memory only and listens on the computer’s loopback address. It answers only
            requests that carry a random 32-byte token, kept in a file only the owner can read. It signs through the
            same wallet rules, refuses anything beyond the remaining amount, and ends when the time is up, when its owner
            locks it, or when the computer restarts.
          </p>
        </Sub>
      </PaperSection>

      <PaperSection id="releases" number={13} title="Software releases">
        <p>
          The <strong>orien</strong> program is published for macOS and Linux on x86-64 and ARM, and for Windows on
          x86-64. Each release
          lists the SHA-256 checksum of every build in a file that is signed with the Orien release key: a key tree of
          height 10, quantum-proof like every other key in the system, whose root is built into the program. The
          signature covers hash(0x47, checksums).
        </p>
        <p>
          <code>orien update</code> installs a newer version only after checking that signature against the built-in
          root and the download against its signed checksum. If the website that serves releases were ever taken over,
          it could not make <code>orien update</code> install anything the release key did not sign.
        </p>
      </PaperSection>

      <PaperSection id="security" number={14} title="Security">
        <Sub title="14.1 Assumptions">
          <ul className="list-disc space-y-2 pl-5">
            <li>
              <strong>BLAKE3 is a secure hash function</strong>: preimages and second preimages cannot be found faster
              than by generic search, and keyed BLAKE3 is a pseudorandom function. The protocol assumes nothing else, except that locked payments
              (section 9) also rely on SHA-256, and that mining relies on RandomX having no shortcut: one would let
              someone earn more of the new coins, never move anyone’s. There is no
              elliptic curve, no factoring and no lattice anywhere in the protocol.
            </li>
            <li>
              <strong>Safety</strong> holds as long as validators holding at most one third of an epoch’s weight
              misbehave in any way.
            </li>
            <li>
              <strong>Progress</strong> needs validators holding more than two thirds of the weight to be honest and
              reachable. Payments need nothing more. Checkpoints, like any agreement protocol, also need the network to
              deliver messages within some bound eventually (partial synchrony [2]).
            </li>
          </ul>
        </Sub>

        <Sub title="14.2 What is guaranteed">
          <ul className="list-disc space-y-2 pl-5">
            <li>
              <strong>No double spending.</strong> Two certificates for conflicting orders would need two quorums, whose
              overlap of more than a third of the stake would have to vote twice (section 4.6).
            </li>
            <li>
              <strong>Nothing final is lost.</strong> A final payment was confirmed by more than two thirds of the stake,
              so at least one honest validator in any checkpoint’s merged statements has it in its pinned log.
            </li>
            <li>
              <strong>One checkpoint per epoch.</strong> Tendermint’s locking rule keeps two different checkpoints from
              being decided for the same epoch.
            </li>
            <li>
              <strong>Cheating is punished.</strong> Voting twice and reusing a one-time key leave signed evidence that
              anyone can check, and a convicted validator loses its stake and its place for good.
            </li>
          </ul>
        </Sub>

        <Sub title="14.3 Quantum computers">
          <p>
            Shor’s algorithm breaks the elliptic-curve signatures most cryptocurrencies use, but not hash functions.
            Against a hash function, a quantum computer can at best search faster with Grover’s algorithm, which halves
            the effective number of bits. Orien’s one-time signatures use the parameters of the one-time layer of
            SLH-DSA-192, NIST’s security category 3, with BLAKE3 in place of SHA-2 or SHAKE: 192-bit chain values,
            leaving about 96 bits against a quantum search. One-time keys add a second layer: an account’s
            next key is revealed only when it signs, and is used for exactly one order, so there is never a long-lived
            public key for an attacker to work on.
          </p>
        </Sub>

        <Sub title="14.4 Attacks considered">
          <ul className="list-disc space-y-2 pl-5">
            <li>
              <strong>A stolen copy of a wallet</strong> can spend. The owner’s best defence is a password on the file
              and the recovery phrase kept on paper. If thief and owner both sign under one number, the account stalls
              until the next checkpoint cancels both. Since the thief’s copy holds the spare keys too, the owner should
              then move what is left to a new wallet.
            </li>
            <li>
              <strong>Flooding</strong> is limited by the minimum fee on every payment, by per-address connection limits
              and timeouts, and by a limit on the shares each validator counts per epoch (section 8.2).
            </li>
            <li>
              <strong>Mining before the start</strong> is impossible: the first seed depends on a Bitcoin block that does
              not exist when the genesis file is published.
            </li>
            <li>
              <strong>A burst of mining power</strong> can earn more coins, but cannot touch payments, and moves the
              target by at most a factor of four per epoch.
            </li>
          </ul>
        </Sub>
      </PaperSection>

      <PaperSection id="measured" number={15} title="Measured performance">
        <p>
          These figures come from validators that sync every vote to disk before answering, with four validators in
          total.
        </p>
        <Table
          head={['Measure', 'Result', 'Conditions']}
          rows={[
            ['Time to finality', '162 ms median (138 to 240 ms)', '40 payments; four validators on three machines, one of them 57 ms away'],
            ['Payments per second', '950 to 1,500', 'four validators on one laptop, 100 to 250 clients at once'],
            ['Payments covered by one validator signature', '10 to 28', 'under load, on one laptop'],
            ['A payment that runs into a checkpoint', 'about 1.1 s', 'on the three machines'],
            ['One-time signature', '1,248 bytes', ''],
            ['Receipt', '12,086 bytes', '3 of 4 votes and 3 of 4 confirmations'],
            ['Checking one signature', 'about 25 µs (40,000 a second)', 'one processor core'],
          ]}
        />
        <p>
          About 115 ms of the 162 ms is the network’s own travel time: a payment needs two round trips to the
          validators, and the rest is signing, checking and syncing to disk.
        </p>
      </PaperSection>

      <PaperSection id="limitations" number={16} title="Known limitations">
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>Nothing is private.</strong> Balances, payments and addresses are public, as in most
            cryptocurrencies, and connections are not encrypted.
          </li>
          <li>
            <strong>Old validators.</strong> Like every system secured by stake, a wallet starting from genesis must
            trust that no group holding more than two thirds of some past epoch’s weight later signed a different
            history. Wallets that keep following the network are not affected.
          </li>
          <li>
            <strong>Restarts during an epoch.</strong> Proof-of-work shares and proofs of cheating are held in memory
            until the checkpoint, so a validator that restarts mid-epoch forgets the ones it alone received.
          </li>
          <li>
            <strong>Spare keys run out.</strong> An account can be cancelled 16 times. After that it stays locked.
          </li>
          <li>
            <strong>Swaps need a wallet that is running.</strong> Each side’s wallet moves a swap along only while it is
            open. A buyer who stays away past the ORN deadline after the seller took the Bitcoin loses the ORN, which
            the deadlines leave more than a day to avoid. The proof of waiting locked payments carries the whole list
            in one message, which outgrows it past about 130,000 of them; until it is split into pages, a buyer then
            relies on the lock’s certificate, which serves only within the epoch the lock was made in.
          </li>
          <li>
            <strong>Large signatures.</strong> Hash-based signatures are about 20 times larger than elliptic-curve ones.
            Batching keeps validators’ share of this small, but each payment carries its own 1,249-byte signature.
          </li>
        </ul>
      </PaperSection>

      <PaperSection id="parameters" number={17} title="Parameters">
        <Table
          head={['Parameter', 'Value']}
          rows={[
            ['Unit', '1 ORN = 10⁹ units'],
            ['Most coins ever', '100,000,000 ORN'],
            ['Half-life of the release', '126,230,400 s (4 × 365.25 days)'],
            ['Coins at the start', '0'],
            ['Epoch', '3,600 s, or 50,000 payments'],
            ['Quorum', 'more than two thirds of the weight (3w > 2·total)'],
            ['Validators', 'at most 33'],
            ['Minimum stake', '10,000 ORN'],
            ['Unstaking', '168 epochs (about a week)'],
            ['Launch seats', '3,000,000 ORN of weight, shared equally, fading as stake arrives'],
            ['Fee', '0.1% of the amount, at least 0.000001 ORN, at most 1 ORN; stake and unstake 0.000001 ORN'],
            ['Shares wanted per epoch', '1,000'],
            ['Proof-of-work', `RandomX, keyed with each epoch’s seed (BLAKE3 before epoch ${RANDOMX_FROM.toLocaleString('en')})`],
            ['First target', `2⁴⁰ − 1; for RandomX, from epoch ${RANDOMX_FROM.toLocaleString('en')}, ⌊(2⁶⁴ − 1) / 400,000⌋`],
            ['Target change per epoch', 'at most × 4 or ÷ 4'],
            ['Shares a validator counts per epoch', '40,000, or 300,000 ÷ validators if fewer'],
            ['Hash function', 'BLAKE3, 32-byte output'],
            ['One-time signature', 'WOTS, w = 16, n = 24 bytes, 51 chains, 1,248 bytes'],
            ['Validator key tree', 'height 16 by default, at most 24'],
            ['Spare keys per account', '16 (key tree of height 4)'],
            ['Recovery phrase', '24 words: 256-bit secret, 8-bit check'],
            ['Wallet password', 'Argon2id (64 MiB, 3 passes), XChaCha20-Poly1305'],
            ['Connection limits', '1,024 in all, 64 per address, 30 s idle'],
            ['States a validator keeps', 'the last 3 epochs'],
            ['Locked payment deadline', '2 to 720 epochs ahead'],
            ['Revealed secrets kept', '168 epochs'],
            ['Swap: ORN lock, Bitcoin lock', '12 epochs, 36 blocks'],
            ['Smallest swap', '15,000 satoshis (0.00015 BTC), more when Bitcoin’s fees are high'],
          ]}
        />
        <p>The domain tags that separate every use of the hash function:</p>
        <Table
          head={['Tag', 'Use', 'Tag', 'Use']}
          rows={[
            ['0x01', 'one-time chain step', '0x34', 'confirmation'],
            ['0x02', 'one-time commitment', '0x35', 'address'],
            ['0x03', 'one-time message', '0x36', 'proposer draw'],
            ['0x10', 'key tree node', '0x37', 'payment log pin'],
            ['0x20–0x22', 'batch tree leaf, node, padding', '0x38', 'statement'],
            ['0x30', 'order', '0x39', 'checkpoint'],
            ['0x31', 'batch message', '0x40–0x42', 'state tree leaf, node, padding'],
            ['0x32', 'genesis (network id)', '0x43', 'ballot'],
            ['0x33', 'account', '0x44', 'proposal'],
            ['0x45', 'stake proof', '0x47', 'software release'],
            ['0x46', 'proof-of-work share', '0x48, 0x49', 'recovery phrase check, account seed'],
            ['0x4B', 'locked payments', '0x4A', 'barred validator keys'],
            ['0x4C', 'revealed secrets', '', ''],
          ]}
        />
      </PaperSection>

      <PaperSection id="references" number={18} title="References">
        <ol className="list-decimal space-y-2 pl-5 text-sm">
          <li>
            M. Baudet, G. Danezis, A. Sonnino. FastPay: High-performance Byzantine fault tolerant settlement. ACM
            Advances in Financial Technologies, 2020.
          </li>
          <li>
            E. Buchman, J. Kwon, Z. Milosevic. The latest gossip on BFT consensus. arXiv:1807.04938, 2018.
          </li>
          <li>
            NIST. FIPS 205: Stateless Hash-Based Digital Signature Standard (SLH-DSA). 2024.
          </li>
          <li>
            R. Merkle. A certified digital signature. CRYPTO 1989. (The Winternitz one-time signature is described
            there.)
          </li>
          <li>Borsh: Binary Object Representation Serializer for Hashing. borsh.io.</li>
          <li>
            J. O’Connor, J.-P. Aumasson, S. Neves, Z. Wilcox-O’Hearn. BLAKE3: one function, fast everywhere. 2020.
          </li>
          <li>R. Merkle. Protocols for public key cryptosystems. IEEE Symposium on Security and Privacy, 1980.</li>
          <li>
            A. Biryukov, D. Dinu, D. Khovratovich, S. Josefsson. RFC 9106: Argon2 memory-hard function for password
            hashing and proof-of-work applications. 2021.
          </li>
          <li>tevador and others. RandomX: proof-of-work algorithm optimized for general-purpose CPUs. github.com/tevador/RandomX, 2019.</li>
        </ol>
      </PaperSection>

      <div className="mb-8 mt-6 flex flex-wrap items-center justify-between gap-4 rounded-2xl border border-line bg-surface p-6 print:hidden">
        <p className="font-semibold text-ink">Prefer the short version?</p>
        <Link
          href="/how-it-works/"
          className="flex h-11 items-center gap-2 rounded-xl border border-line bg-bg px-5 text-sm font-semibold text-ink transition hover:border-pearl"
        >
          How it works <ArrowRight className="h-4 w-4" aria-hidden />
        </Link>
      </div>
    </article>
  )
}
