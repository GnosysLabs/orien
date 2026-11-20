// Real answers from the explorer's API on 2026-11-20, shortened: long lists
// keep only their first items. Shown on the API page.

export const EXAMPLES = {
  overview: `{
  "epoch": 137,
  "epoch_started": 1795184035,
  "epoch_seconds": 3600,
  "mining": "RandomX",
  "validators": [
    {
      "seat": 0,
      "name": "orien-1",
      "up": true,
      "epoch": 137,
      "agreed": 24,
      "of": 24,
      "stake": "750000000000000",
      "account": null
    }
  ],
  "supply": {
    "cap": "100000000000000000",
    "released": "270549096229967",
    "per_hour": "1971437603824",
    "in_wallets": "270547473113158",
    "staked": "0",
    "stakers": 0,
    "holding": 133,
    "holding_one": 121,
    "burned": "0"
  },
  "miners": {
    "epoch": 136,
    "last_hour": 85,
    "last_day": 112,
    "hours": 24
  },
  "hashrate": {
    "epoch": 136,
    "per_second": 3584873.4867831175,
    "hours": 6
  },
  "checkpoints": [
    {
      "epoch": 136,
      "time": 1795184035,
      "payments": 0,
      "released": "1971477041204",
      "agreed": [
        0,
        1,
        2,
        3
      ],
      "of": 4,
      "round": 0,
      "state": "1cc560c5940ea85736f042e52cbd189a47769c5c2f7a785208b47a6413a2a5b1",
      "digest": "40fdf4bde7c6f3d48f1af468d428f63c4e70caed3f21533b200d42d0c0e1b2ba"
    }
  ]
}`,
  checkpoints: `{
  "checkpoints": [
    {
      "agreed": [
        0,
        1,
        2,
        3
      ],
      "digest": "40fdf4bde7c6f3d48f1af468d428f63c4e70caed3f21533b200d42d0c0e1b2ba",
      "epoch": 136,
      "of": 4,
      "payments": 0,
      "released": "1971477041204",
      "round": 0,
      "state": "1cc560c5940ea85736f042e52cbd189a47769c5c2f7a785208b47a6413a2a5b1",
      "time": 1795184035
    }
  ],
  "more": true
}`,
  checkpoint: `{
  "checkpoint": {
    "agreed": [
      0,
      1,
      2,
      3
    ],
    "digest": "40fdf4bde7c6f3d48f1af468d428f63c4e70caed3f21533b200d42d0c0e1b2ba",
    "epoch": 136,
    "of": 4,
    "payments": 0,
    "released": "1971477041204",
    "round": 0,
    "state": "1cc560c5940ea85736f042e52cbd189a47769c5c2f7a785208b47a6413a2a5b1",
    "time": 1795184035
  },
  "latest": 136,
  "previous": "478f68a5d39a024a81ad25e272e66127e1bdd552cdaf7d79bb33b2e33b5b95e8",
  "rewards": {
    "fees": "0",
    "harder": -0.0020599999999983964,
    "known": true,
    "math": "RandomX",
    "mined": "1971477041201",
    "miners": [
      {
        "address": "2b6381f5ca71660bd93c19f8a5cbd2d675a7146546d43b18573734aca9a23ccd",
        "amount": "741304131467",
        "part": 0.3760145900636035
      },
      {
        "address": "fb733425f7c7bd92322d75bed62e6ea98e62e3b689a43172fc6ef4d5e641050a",
        "amount": "96406677368",
        "part": 0.048900735516184464
      }
    ],
    "since": 12,
    "stakers": []
  },
  "validators": [
    {
      "account": null,
      "agreed": true,
      "seat": 0,
      "stake": "750000000000000"
    }
  ]
}`,
  address: `{
  "address": "e8bd81e7e7ae832540f4976e51ff9ece6c015620557900b097bdfeffc1411225",
  "balance": "40193878403538",
  "staked": "0",
  "returning": "0",
  "payments_made": 0,
  "activity": [
    {
      "epoch": 121,
      "time": 1795130015,
      "kind": "mined",
      "amount": "1390874911",
      "counterparty": null
    },
    {
      "epoch": 120,
      "time": 1795126415,
      "kind": "mined",
      "amount": "607672836",
      "counterparty": null
    }
  ],
  "more": false,
  "since": 12,
  "since_time": 1794737465
}`,
  top: `{
  "epoch": 137,
  "holders": [
    {
      "address": "e8bd81e7e7ae832540f4976e51ff9ece6c015620557900b097bdfeffc1411225",
      "holds": "40193878403538",
      "sent": 0,
      "share": 0.14856497435009006,
      "staked": "0"
    }
  ],
  "holding": 133,
  "in_wallets": "270547473113158"
}`,
}
