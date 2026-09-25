# Fixture provenance

Every program fixture in this crate was either read off a Kaspa chain or built from published source. This file says which, so each one can be checked without trusting kascov. Full transaction and covenant IDs are in [`provenance.json`](provenance.json); the tables below show the first 16 hex characters.

## How to check a fixture

1. Fetch the transaction in the *revealed by* column from your own Kaspa node, or any explorer.
2. Its input *N* spends the outpoint shown, and that input's signature script ends with a push of the program.
3. The pushed bytes must equal the fixture file byte for byte. Their BLAKE2b-256 must equal the fixture's hash, and the spent output's script is `aa20 <hash> 87`.

The `kascov-decode` tool does steps 2 and 3 in one command:

```bash
kascov-decode check <signature script hex> --spk <spent output script hex>
```

## Summary

Checked against kascov's own index of mainnet and testnet-10 on 2026-09-25.

| What | Fixtures |
|---|---|
| Program on chain, and a spend reveals exactly these bytes | 254 |
| Whole signature script found on chain | 9 |
| Not a whole on-chain output (template, fragment, or built from source), origin noted | 16 |

## Programs revealed on chain

*Outputs* counts every output on that network that commits to the program. *Verified* counts the spends of those outputs whose final push is exactly the fixture.

| File | Bytes | Network | Outputs | Verified | Spent outpoint | Revealed by (input) |
|---|---|---|---|---|---|---|
| `fixtures/allowance_tn_f_a.bin` | 589 | testnet-10 | 3972 | 3960 of 3960 | `16fc34c9988241a9:0` | `c6a65bd972b9d614` (None) |
| `fixtures/allowance_tn_f_b.bin` | 589 | testnet-10 | 2267 | 2260 of 2260 | `a86cc51aa3472fa8:0` | `9a9744b411bdf029` (None) |
| `fixtures/allowance_tn_f_c.bin` | 589 | testnet-10 | 2 | 2 of 2 | `4a1d29815ad07cf1:0` | `4ee068d9c3c44c9b` (None) |
| `fixtures/g0_auction_a.bin` | 1674 | testnet-10 | 3 | 1 of 1 | `a3985e3b94a532b4:0` | `7d75e1568d51618c` (0) |
| `fixtures/g0_auction_b.bin` | 1674 | testnet-10 | 1 | 1 of 1 | `eb545ef546aa83bf:0` | `40ba90450c88b317` (0) |
| `fixtures/g0_buy_v1_a.bin` | 471 | testnet-10 | 3 | 3 of 3 | `377f4ad75a1ae50d:0` | `74452c1d6a4a7e50` (None) |
| `fixtures/g0_buy_v1_b.bin` | 471 | testnet-10 | 1 | 1 of 1 | `e9bbaeaf928d09ed:0` | `88f4888a1ba0c8f5` (None) |
| `fixtures/g0_buy_v2_a.bin` | 369 | testnet-10 | 1 | 1 of 1 | `7bca5ebc293824e0:0` | `809fd699b3403871` (None) |
| `fixtures/g0_buy_v2_b.bin` | 369 | testnet-10 | 1 | 1 of 1 | `d126149b90abb7b0:0` | `e91f6a3de70437ba` (None) |
| `fixtures/g0_buy_v3_a.bin` | 471 | testnet-10 | 1 | 1 of 1 | `71395898b9a5833c:0` | `5778a4da44b5a49f` (0) |
| `fixtures/g0_buy_v3_b.bin` | 471 | testnet-10 | 1 | 1 of 1 | `a151e3eb93c61ae6:0` | `1300bc7ed7466836` (0) |
| `fixtures/g0_col_a.bin` | 83 | testnet-10 | 17 | 7 of 9 | `8e227a370a3095c8:0` | `52263de78be991c3` (None) |
| `fixtures/g0_col_b.bin` | 83 | testnet-10 | 698 | 628 of 628 | `64d2d9b6c932eda8:0` | `9cf4af1f8b6f8910` (None) |
| `fixtures/g0_dutch_list_a.bin` | 1024 | testnet-10 | 1 | 1 of 1 | `201db2bf8239ae16:0` | `2ff4277a853f5e19` (0) |
| `fixtures/g0_dutch_list_b.bin` | 1024 | testnet-10 | 1 | 1 of 1 | `daa08d2a4e0ba9e2:0` | `95575e7b1708b5c6` (0) |
| `fixtures/g0_krc_gate_active_a.bin` | 105 | testnet-10 | 1 | 1 of 1 | `208604824b930723:3` | `a2f4bc55655fea06` (2) |
| `fixtures/g0_krc_gate_active_b.bin` | 105 | testnet-10 | 1 | 1 of 1 | `41e663244d7a8279:3` | `fbd69054c762e216` (2) |
| `fixtures/g0_krc_gate_boot_a.bin` | 101 | testnet-10 | 1 | 1 of 1 | `737f74b274c47930:0` | `f3e5a5d8f1df031b` (0) |
| `fixtures/g0_krc_gate_boot_b.bin` | 101 | testnet-10 | 1 | 1 of 1 | `8c0121e6dae03d58:0` | `ef371702a7300468` (0) |
| `fixtures/g0_list_v1_a.bin` | 1024 | testnet-10 | 1 | 1 of 1 | `abdfdcebbc8bb6cb:0` | `7b75396e57f5688a` (None) |
| `fixtures/g0_list_v1_b.bin` | 1024 | testnet-10 | 1 | 1 of 1 | `64ea44cf7020870e:1` | `8f864739684925f5` (None) |
| `fixtures/g0_list_v2_a.bin` | 600 | testnet-10 | 1 | 1 of 1 | `c9ae0d4640682ffe:1` | `7bca5ebc293824e0` (None) |
| `fixtures/g0_list_v2_b.bin` | 600 | testnet-10 | 1 | 1 of 1 | `134714802c75cce2:1` | `b8de399c83906b15` (None) |
| `fixtures/g0_list_v3_a.bin` | 1024 | testnet-10 | 1 | 1 of 1 | `e8688e9e37b7cafa:1` | `361ec933d82ed451` (0) |
| `fixtures/g0_list_v3_b.bin` | 1024 | testnet-10 | 1 | 1 of 1 | `e72252bb3fad539f:1` | `abfaf2f484a68859` (0) |
| `fixtures/g0_mint_560_a.bin` | 560 | testnet-10 | 16 | 15 of 15 | `e074a1c46cf4253e:0` | `b6d89653a5482f50` (0) |
| `fixtures/g0_mint_560_b.bin` | 560 | testnet-10 | 16 | 15 of 15 | `a5f0fafde7aa2f1c:0` | `182c79b2c12b8e5e` (0) |
| `fixtures/g0_root_a.bin` | 56 | testnet-10 | 1 | 1 of 1 | `d7740341a3af79a0:0` | `9350e2b1d00417fa` (None) |
| `fixtures/g0_root_b.bin` | 56 | testnet-10 | 149 | 142 of 142 | `bd9b1030507ae428:0` | `39936c3ec2c0abf7` (None) |
| `fixtures/g0_slot_mint_413_a.bin` | 413 | testnet-10 | 2 | 1 of 1 | `7074aba4e101f699:0` | `c099f6406b6f3ac7` (None) |
| `fixtures/g0_slot_mint_413_b.bin` | 413 | testnet-10 | 3 | 2 of 2 | `d18931f605b83be4:0` | `f15180bfd29965bc` (None) |
| `fixtures/g0_slot_mint_421_a.bin` | 421 | testnet-10 | 2 | 1 of 1 | `ab463624894ef528:0` | `42120a3a7f229dec` (None) |
| `fixtures/g0_slot_mint_421_b.bin` | 421 | testnet-10 | 2 | 1 of 1 | `9dbd51b686d79414:0` | `b1fbe216098d6eb5` (None) |
| `fixtures/g0_slot_mint_520_a.bin` | 520 | testnet-10 | 2 | 1 of 1 | `c3f001730e554ae6:0` | `bf8a83b8cdea5ca8` (0) |
| `fixtures/g0_slot_mint_520_b.bin` | 520 | testnet-10 | 9 | 8 of 8 | `7f88d388bbc790a1:0` | `eba2253ee6d2ecac` (0) |
| `fixtures/g0_slot_mint_565_a.bin` | 565 | testnet-10 | 3 | 2 of 2 | `a025eb178407441c:0` | `8761d0b5d928ebcc` (0) |
| `fixtures/g0_slot_mint_565_b.bin` | 565 | testnet-10 | 5 | 4 of 4 | `d5d7095315dd188a:0` | `ca848a684ae0b0fa` (0) |
| `fixtures/g0_style_list_a.bin` | 1024 | testnet-10 | 1 | 1 of 1 | `5de8a9e74bedbcad:1` | `11e7d39a3c80e8d9` (0) |
| `fixtures/g0_style_list_b.bin` | 1024 | testnet-10 | 1 | 1 of 1 | `e2dc1e2533dd5f25:1` | `2620ff4537b1af9f` (0) |
| `fixtures/kaha_pool_102467_a.bin` | 102467 | testnet-10 | 1 | 1 of 1 | `2616d4c50f16705f:0` | `2dc564fdec2857e6` (0) |
| `fixtures/kaha_pool_102467_b.bin` | 102467 | testnet-10 | 1 | 1 of 1 | `081e570d579a17d4:0` | `4c6fdbf34742633e` (0) |
| `fixtures/kaha_pool_102564_a.bin` | 102564 | testnet-10 | 1 | 1 of 1 | `12b8b0cd89d74a29:0` | `55683bd585c77f75` (0) |
| `fixtures/kaha_pool_102564_b.bin` | 102564 | testnet-10 | 1 | 1 of 1 | `d405c97fcea69402:0` | `d02d37436a5ac600` (0) |
| `fixtures/kaha_pool_108623_a.bin` | 108623 | testnet-10 | 1 | 1 of 1 | `ee71d78023a24a26:0` | `d5b7f238e1b7348e` (0) |
| `fixtures/kaha_pool_108623_b.bin` | 108623 | testnet-10 | 1 | 1 of 1 | `f1a5823e1c677a46:0` | `d2db7cdbf06acda7` (0) |
| `fixtures/kaha_pool_108714_a.bin` | 108714 | testnet-10 | 1 | 1 of 1 | `9ad8f47e861e19d6:0` | `4b933e1a8e2fca93` (0) |
| `fixtures/kaha_pool_108714_b.bin` | 108714 | testnet-10 | 1 | 1 of 1 | `022a90d1aaa1d1f0:0` | `630d0da3e27df5d2` (0) |
| `fixtures/kaha_pool_108722_a.bin` | 108722 | testnet-10 | 1 | 1 of 1 | `1975ed08fcc7084a:0` | `499042c8352dcb6c` (0) |
| `fixtures/kaha_pool_108722_b.bin` | 108722 | testnet-10 | 1 | 1 of 1 | `7dfc362581edb3fd:0` | `7d4e0a22c06c1bed` (0) |
| `fixtures/kaha_pool_68511_a.bin` | 68511 | testnet-10 | 1 | 1 of 1 | `94056a9a6395ec8c:0` | `d01865c1739183d1` (0) |
| `fixtures/kaha_pool_68511_b.bin` | 68511 | testnet-10 | 1 | 1 of 1 | `aaa43643a84f5663:0` | `51eb0a2e296bc936` (0) |
| `fixtures/kaha_pool_68525_a.bin` | 68525 | testnet-10 | 1 | 1 of 1 | `518953358fe08f25:0` | `2f9e6a44fb008b7a` (0) |
| `fixtures/kaha_pool_68525_b.bin` | 68525 | testnet-10 | 1 | 1 of 1 | `93f0a921bca3f1d2:0` | `34ca997517c00f6e` (0) |
| `fixtures/kaha_pool_75692_a.bin` | 75692 | testnet-10 | 1 | 1 of 1 | `e7f6dfdac4129ac2:0` | `2d1e44fad8f48dca` (0) |
| `fixtures/kaha_pool_75692_b.bin` | 75692 | testnet-10 | 1 | 1 of 1 | `7abc88cc4169632a:0` | `d9e40de383c25aa8` (0) |
| `fixtures/kaha_pool_92140_a.bin` | 92140 | testnet-10 | 1 | 1 of 1 | `ec6c6c244b820a0d:0` | `868c5cd07ca9acbd` (0) |
| `fixtures/kaha_pool_92140_b.bin` | 92140 | testnet-10 | 1 | 1 of 1 | `bd6bd6f2dc2feaac:0` | `e7a419aed3d6ca2c` (0) |
| `fixtures/kaha_pool_93943_a.bin` | 93943 | testnet-10 | 1 | 1 of 1 | `05e04a8834e4fc60:0` | `85c0c444b8ff1811` (0) |
| `fixtures/kaha_pool_93943_b.bin` | 93943 | testnet-10 | 1 | 1 of 1 | `891c2edf82d5f8c2:0` | `8a71b559789ec694` (0) |
| `fixtures/kasparocket_lp_amm_a.bin` | 22861 | testnet-10 | 1 | 1 of 1 | `6c424ab860c146e7:3` | `fcafdfb77c5052b4` (1) |
| `fixtures/kasparocket_lp_amm_a_graduated.bin` | 22861 | testnet-10 | 1 | 1 of 1 | `2658fdd99665be9a:1` | `22ef0ab20ff96ac1` (0) |
| `fixtures/kasparocket_lp_amm_b.bin` | 18062 | testnet-10 | 65 | 64 of 64 | `9d60846a7a6b46fc:0` | `1e29bb9bdd7a94be` (0) |
| `fixtures/kasparocket_lp_amm_b_bonding.bin` | 18062 | testnet-10 | 1 | 1 of 1 | `fa84629ab963ed2a:0` | `695595092cbc500a` (0) |
| `fixtures/kcc0020/zealous_token_2641_a.bin` | 2641 | testnet-10 | 2 | 2 of 2 | `915ddf6263f569c8:0` | `2cfffc4c7bdaf488` (0) |
| `fixtures/kcc0020/zealous_token_2641_b.bin` | 2641 | testnet-10 | 1 | 1 of 1 | `694e15708f8c1cc8:0` | `187a4a8c1fabcf76` (1) |
| `fixtures/kcc0020/zealous_token_3037_a.bin` | 3037 | testnet-10 | 2 | 2 of 2 | `ca165ca277b7ab16:0` | `0be649afbea2b0dd` (0) |
| `fixtures/kcc0020/zealous_token_3037_b.bin` | 3037 | testnet-10 | 1 | 1 of 1 | `0bcf537928a75005:0` | `33bb6332e6060d20` (1) |
| `fixtures/kcc0020/zealous_token_3223_a.bin` | 3223 | testnet-10 | 2 | 2 of 2 | `13048c7fe75b4807:0` | `0a847df8e16420ed` (0) |
| `fixtures/kcc0020/zealous_token_3223_b.bin` | 3223 | testnet-10 | 1 | 1 of 1 | `d1b71be71be2ec44:4` | `641451d7ea00efef` (1) |
| `fixtures/kcc0020/zealous_token_a.bin` | 2590 | testnet-10 | 1 | 1 of 1 | `8df38dc9d0bd7b2f:1` | `02325f2008380508` (1) |
| `fixtures/kcc0020/zealous_token_b.bin` | 2590 | testnet-10 | 1 | 1 of 1 | `1cd74a9f93897b54:1` | `c5d95e84921b1f3d` (0) |
| `fixtures/kcc20_a_a.bin` | 1568 | testnet-10 | 9 | 7 of 7 | `194cbc1b10f84333:4` | `6b533050e3cb57e1` (None) |
| `fixtures/kcc20_a_b.bin` | 1568 | testnet-10 | 2 | 1 of 1 | `99d6800703d25f38:0` | `5ec15621407257e9` (None) |
| `fixtures/kcc20_b_a.bin` | 2728 | testnet-10 | 3 | 2 of 2 | `289c0a9114b7f689:0` | `b4b796ebef8a8a15` (None) |
| `fixtures/kcc20_b_b.bin` | 2728 | testnet-10 | 1 | 1 of 1 | `b4b796ebef8a8a15:1` | `57130dc9b11f9f7f` (None) |
| `fixtures/kcc20_c_a.bin` | 1540 | testnet-10 | 1 | 1 of 1 | `792b50c9c873d371:0` | `e37e91cdf55e6290` (None) |
| `fixtures/kcc20_c_b.bin` | 1540 | testnet-10 | 2 | 1 of 1 | `ebc07828728c6cef:0` | `fdcd2464bbcf1958` (None) |
| `fixtures/kcc20_minter_a.bin` | 6201 | testnet-10 | 3 | 2 of 2 | `253f4dd6bb1d9a7c:0` | `f107015c2fde68fa` (None) |
| `fixtures/kcc20_minter_b.bin` | 6201 | testnet-10 | 125 | 124 of 124 | `ca13b9315eb1b08c:0` | `7099d0c56c61af9e` (None) |
| `fixtures/kcc20_unguarded_kron.bin` | 2433 | mainnet | 3 | 3 of 3 | `10f70b384ada729a:4` | `836d0e14407d5a6f` (2) |
| `fixtures/kcm_curve_live_tn10.bin` | 17269 | testnet-10 | 2 | 2 of 2 | `75994f84026acbcf:0` | `4d236eef3bc23de9` (0) |
| `fixtures/kcom_token_2671_a.bin` | 2671 | testnet-10 | 2 | 2 of 2 | `b4478cec21668548:2` | `e9d5213f08cdbab1` (None) |
| `fixtures/kcom_token_2671_b.bin` | 2671 | testnet-10 | 2 | 1 of 1 | `258108069cd996e3:0` | `a4c3adf879e783c7` (None) |
| `fixtures/kcom_token_a.bin` | 8076 | testnet-10 | 3 | 3 of 3 | `e5f261b2de7b043c:2` | `bc3f2b69963b56eb` (2) |
| `fixtures/kcom_token_b.bin` | 8076 | testnet-10 | 1 | 1 of 1 | `4bf64ad3e6047e3b:0` | `866e5c18a5392438` (0) |
| `fixtures/kcom_wrapped_10094_a.bin` | 10094 | testnet-10 | 1 | 1 of 1 | `d5dfc0000d70cb50:0` | `987ad1131fbd81dc` (None) |
| `fixtures/kcom_wrapped_10094_b.bin` | 10094 | testnet-10 | 17 | 16 of 16 | `ca7a39676359f74b:1` | `68d1a06f11400679` (None) |
| `fixtures/kcom_wrapped_14258_a.bin` | 14258 | testnet-10 | 3 | 3 of 3 | `2b60cefb7bc46e9b:0` | `ba6ca67b478753b4` (0) |
| `fixtures/kcom_wrapped_14258_b.bin` | 14258 | testnet-10 | 6 | 5 of 5 | `222d3fff5f8a8230:1` | `71ca7fc44959e605` (0) |
| `fixtures/kcom_wrapped_14273_a.bin` | 14273 | testnet-10 | 23 | 22 of 22 | `c1126a952c9da7cb:1` | `258972ce9a931489` (0) |
| `fixtures/kcom_wrapped_14273_b.bin` | 14273 | testnet-10 | 1 | 1 of 1 | `0664153032d54ff8:7` | `bf7a2c1b280141e2` (3) |
| `fixtures/kcom_wrapped_16984_a.bin` | 16984 | testnet-10 | 4 | 4 of 4 | `e5f261b2de7b043c:0` | `6681401c041fa690` (0) |
| `fixtures/kcom_wrapped_16984_b.bin` | 16984 | testnet-10 | 1 | 1 of 1 | `8200b984e16fab42:1` | `be620da533d2ad20` (0) |
| `fixtures/kcom_wrapped_22775_a.bin` | 22775 | testnet-10 | 2 | 1 of 1 | `62234f2c6cdf6f88:1` | `ba3055e1ac6657bc` (0) |
| `fixtures/kcom_wrapped_22775_b.bin` | 22775 | testnet-10 | 2 | 2 of 2 | `411dfceba942e299:0` | `55f4b620c79bbb3f` (0) |
| `fixtures/kcom_wrapped_25496_a.bin` | 25496 | testnet-10 | 1 | 1 of 1 | `a652631f33179658:0` | `ef916189cccf1ed5` (1) |
| `fixtures/kcom_wrapped_25496_b.bin` | 25496 | testnet-10 | 2 | 1 of 1 | `1e3dcef26d13851a:0` | `fa043e35cc9da12a` (0) |
| `fixtures/kcom_wrapped_7273_a.bin` | 7273 | testnet-10 | 8 | 7 of 7 | `e4e06ab72740b222:1` | `16db664ecec9e2bf` (None) |
| `fixtures/kcom_wrapped_7273_b.bin` | 7273 | testnet-10 | 1 | 1 of 1 | `ab6bbe8a2f63e259:0` | `83a65c1186b7b4fe` (None) |
| `fixtures/kcom_wrapped_9889_a.bin` | 9889 | testnet-10 | 1 | 1 of 1 | `a3a8aa0956a679cd:0` | `7c3a9026d66f031c` (0) |
| `fixtures/kcom_wrapped_9889_b.bin` | 9889 | testnet-10 | 1 | 1 of 1 | `79bc6a6327480efb:0` | `5315c57c632e1159` (None) |
| `fixtures/kcom_wrapper_3776_a.bin` | 3776 | testnet-10 | 3 | 2 of 2 | `c9adf50aeadb9b02:0` | `2d078c5c9bcfb906` (0) |
| `fixtures/kcom_wrapper_3776_b.bin` | 3776 | testnet-10 | 3 | 2 of 2 | `26f9e14e6800da14:0` | `95bb5e3a2a900cd2` (0) |
| `fixtures/kcom_wrapper_3796_a.bin` | 3796 | testnet-10 | 3 | 2 of 2 | `62234f2c6cdf6f88:0` | `094853793ff17afe` (0) |
| `fixtures/kcom_wrapper_3796_b.bin` | 3780 | testnet-10 | 3 | 2 of 2 | `bd55b26c9f1b0389:0` | `497306ba7beab156` (0) |
| `fixtures/kcom_wrapper_3807_a.bin` | 3807 | testnet-10 | 2 | 1 of 1 | `524752921b1f9061:0` | `6a5b77d3e4d13edf` (0) |
| `fixtures/kcom_wrapper_3807_b.bin` | 3783 | testnet-10 | 2 | 1 of 1 | `e4e06ab72740b222:0` | `aedac19e4853eafe` (None) |
| `fixtures/kcom_wrapper_3895_a.bin` | 3895 | testnet-10 | 6 | 5 of 5 | `f544d7c56de27e04:0` | `4128e7a5e211eb71` (0) |
| `fixtures/kcom_wrapper_3895_b.bin` | 3895 | testnet-10 | 3 | 2 of 2 | `b0de71c44588083d:0` | `66c7150826454a9d` (0) |
| `fixtures/kforge_curve_5900_a.bin` | 5900 | testnet-10 | 8 | 2 of 2 | `cf6d24f9248c0192:0` | `0a83228eae4d78bc` (0) |
| `fixtures/kforge_curve_5900_b.bin` | 5900 | testnet-10 | 1 | 1 of 1 | `adeb354c008059b1:2` | `cfd928fb579664a9` (0) |
| `fixtures/kforge_pool_1370_a.bin` | 1370 | testnet-10 | 1 | 1 of 1 | `8a2a6e09658d19a6:0` | `93afc12a2b8fce5f` (0) |
| `fixtures/kforge_pool_1370_b.bin` | 1370 | testnet-10 | 1 | 1 of 1 | `32409faa2c41bd51:0` | `3e29ded886db0ce5` (0) |
| `fixtures/kforge_token_1785_a.bin` | 1785 | testnet-10 | 2 | 1 of 1 | `fbfa34b1808df4e2:0` | `33ccee97ba0753ff` (0) |
| `fixtures/kforge_token_1785_b.bin` | 1785 | testnet-10 | 2 | 2 of 2 | `7494882904f8174b:5` | `5aa0df8bad0a9d22` (2) |
| `fixtures/kr_lpamm_18060_a.bin` | 18060 | testnet-10 | 1 | 1 of 1 | `fe45ff3ade339178:0` | `589dc5092a548fe7` (None) |
| `fixtures/kr_lpamm_18060_b.bin` | 18060 | testnet-10 | 1 | 1 of 1 | `88184f83ae237b31:0` | `a8cb0c21880d17fe` (None) |
| `fixtures/kr_lpamm_18062_a.bin` | 18062 | testnet-10 | 65 | 64 of 64 | `9d60846a7a6b46fc:0` | `1e29bb9bdd7a94be` (0) |
| `fixtures/kr_lpamm_18062_b.bin` | 18062 | testnet-10 | 1 | 1 of 1 | `3f344a69fec52aa4:0` | `cbe79353b813cb9f` (0) |
| `fixtures/kr_lpamm_22861_a.bin` | 22861 | testnet-10 | 1 | 1 of 1 | `356261babc8427bc:1` | `c497850ad76634a3` (0) |
| `fixtures/kr_lpamm_22861_b.bin` | 22861 | testnet-10 | 1 | 1 of 1 | `3f09bd6f753ea584:0` | `37354df64db5e9aa` (0) |
| `fixtures/kr_order_escrow_43641_a.bin` | 43641 | testnet-10 | 1 | 1 of 1 | `6c424ab860c146e7:0` | `a1308a3de67046be` (0) |
| `fixtures/kr_order_escrow_43641_b.bin` | 43641 | testnet-10 | 1 | 1 of 1 | `5c63033ccf945f7d:0` | `3f7a476682c32be9` (0) |
| `fixtures/kr_order_escrow_65541_a.bin` | 65541 | testnet-10 | 1 | 1 of 1 | `542d006911c35fa7:0` | `7f0995763f267fe7` (0) |
| `fixtures/kr_order_escrow_65541_b.bin` | 65541 | testnet-10 | 1 | 1 of 1 | `2f0f55ada92e9e9e:2` | `57ef2918390033e3` (0) |
| `fixtures/kron_curve_144959_a.bin` | 145951 | testnet-10 | 2 | 2 of 2 | `065572c0299f8423:0` | `ad770a66ac128e93` (None) |
| `fixtures/kron_curve_144959_b.bin` | 145951 | testnet-10 | 1 | 1 of 1 | `da4f263c92411e14:0` | `12550ec544bc3356` (None) |
| `fixtures/kron_curve_144959_c.bin` | 145951 | testnet-10 | 1 | 1 of 1 | `ee35e53fa0bcfa48:0` | `791b30a1a9e8381b` (None) |
| `fixtures/kron_curve_144959_d.bin` | 144959 | testnet-10 | 1 | 1 of 1 | `e96c6d607d8d5e54:0` | `5d59f47d926c6a56` (None) |
| `fixtures/kron_curve_153859_a.bin` | 153877 | testnet-10 | 1 | 1 of 1 | `e36e0632d64e4a20:0` | `76f02fdae045c552` (None) |
| `fixtures/kron_curve_153859_b.bin` | 153877 | testnet-10 | 1 | 1 of 1 | `76f02fdae045c552:0` | `7fa9ac4a9d36a75f` (None) |
| `fixtures/kron_curve_153859_c.bin` | 153869 | testnet-10 | 1 | 1 of 1 | `8d4cff09dc68b00f:0` | `90a2daf5a886c675` (None) |
| `fixtures/kron_curve_171749_a.bin` | 171761 | testnet-10 | 1 | 1 of 1 | `4a8db07e49f26eb5:0` | `d62439095348fc5e` (0) |
| `fixtures/kron_curve_171749_b.bin` | 171761 | testnet-10 | 1 | 1 of 1 | `d62439095348fc5e:0` | `ec9766005704e7e1` (0) |
| `fixtures/kron_curve_171749_c.bin` | 171749 | testnet-10 | 2 | 2 of 2 | `5b94d3ddc2cfa3b5:0` | `e08c2a4b210853fa` (0) |
| `fixtures/kron_curve_171749_d.bin` | 171749 | testnet-10 | 1 | 1 of 1 | `c5eb93b2752f04f8:0` | `82199b827c8172b5` (None) |
| `fixtures/kron_curve_185912_a.bin` | 185912 | testnet-10 | 1 | 1 of 1 | `63792ab4277fee10:0` | `0ba71e01b38c4ece` (0) |
| `fixtures/kron_curve_185912_b.bin` | 185912 | testnet-10 | 1 | 1 of 1 | `0ba71e01b38c4ece:0` | `dd3ee423d8ef0022` (0) |
| `fixtures/kron_curve_185912_c.bin` | 185937 | testnet-10 | 1 | 1 of 1 | `756aecfc5f7f6ca2:0` | `913134cf1bcabf66` (0) |
| `fixtures/kron_curve_tn_a_sibling.bin` | 153869 | testnet-10 | 1 | 1 of 1 | `95f7c6e6d840554b:0` | `84329bfe79323d05` (None) |
| `fixtures/kron_curve_v1.bin` | 172161 | mainnet | 2 | 2 of 2 | `2d2a36ba9eaf4b77:0` | `138162b905700138` (0) |
| `fixtures/kron_curve_v2.bin` | 185750 | mainnet | 1 | 1 of 1 | `7e96fbb1eba3fa11:0` | `b8ec94e30f6f961c` (0) |
| `fixtures/kron_curve_v3.bin` | 185891 | mainnet | 1 | 1 of 1 | `a8cc6bfeca330d92:0` | `ad016e8d02b933ef` (0) |
| `fixtures/kron_curve_v4.bin` | 185925 | mainnet | 1 | 1 of 1 | `cff7bf031df03140:0` | `8b128b5e9dedf25d` (0) |
| `fixtures/kron_curve_v5.bin` | 207136 | mainnet | 1 | 1 of 1 | `73f4a618489994f0:0` | `5465ffe226b48629` (0) |
| `fixtures/kron_curve_v5_lookalike.bin` | 186326 | mainnet | 1 | 1 of 1 | `08530605bdcb04f8:0` | `d53cd649a2323c49` (0) |
| `fixtures/kron_curve_v5_sonar.bin` | 207136 | mainnet | 1 | 1 of 1 | `6cbad6cc44a3cf02:0` | `5d979a7602a9815b` (0) |
| `fixtures/kron_pool_50880a_a.bin` | 51232 | testnet-10 | 1 | 1 of 1 | `a95c2c91bb363b4c:1` | `a681e140371b3268` (None) |
| `fixtures/kron_pool_50880a_b.bin` | 51232 | testnet-10 | 1 | 1 of 1 | `6292d0b10625fe85:0` | `062da3d44032a9ee` (None) |
| `fixtures/kron_pool_50880a_c.bin` | 50880 | testnet-10 | 1 | 1 of 1 | `9a4ee57e1840dc63:1` | `c3fee59e5c776351` (None) |
| `fixtures/kron_pool_50880a_d.bin` | 51232 | testnet-10 | 1 | 1 of 1 | `062da3d44032a9ee:0` | `3561a19ba72a4f86` (None) |
| `fixtures/kron_pool_50880b_a.bin` | 50880 | testnet-10 | 1 | 1 of 1 | `ca446c8f55a95760:1` | `cdf32cf4b62d5d8a` (None) |
| `fixtures/kron_pool_50880b_b.bin` | 50880 | testnet-10 | 1 | 1 of 1 | `3d4cf9d8699fdd66:0` | `d279fc77b0e2c42b` (None) |
| `fixtures/kron_pool_50880b_c.bin` | 50880 | testnet-10 | 1 | 1 of 1 | `1528843638348dbc:0` | `1886ded5a03c3e9c` (None) |
| `fixtures/kron_pool_52677_a.bin` | 52677 | testnet-10 | 1 | 1 of 1 | `f66bc6f43c7376a5:1` | `5306faa0d8acab02` (None) |
| `fixtures/kron_pool_52677_b.bin` | 52677 | testnet-10 | 1 | 1 of 1 | `087176ea7f7cbfa7:0` | `edb68fcbb501c1cb` (None) |
| `fixtures/kron_pool_52677_c.bin` | 52677 | testnet-10 | 1 | 1 of 1 | `2d707242778a1a5f:0` | `f92984f3eafe71a1` (None) |
| `fixtures/kron_pool_59183_a.bin` | 59183 | testnet-10 | 1 | 1 of 1 | `ca5c602214fa9cef:1` | `9c9093a53194b1a5` (0) |
| `fixtures/kron_pool_59183_b.bin` | 59183 | testnet-10 | 1 | 1 of 1 | `8892c86de5d61cc6:0` | `e515c7e73763a281` (0) |
| `fixtures/kron_pool_59183_c.bin` | 59183 | testnet-10 | 1 | 1 of 1 | `5ed1bd0683e07569:1` | `9896412e84553e95` (0) |
| `fixtures/kron_pool_59183_d.bin` | 59183 | testnet-10 | 1 | 1 of 1 | `e515c7e73763a281:0` | `b9d8863cf7d74dd9` (0) |
| `fixtures/kron_pool_v2_liq_add_base.bin` | 59126 | mainnet | 1 | 1 of 1 | `66ad481faae88acb:0` | `cdd39382aa7c7e78` (0) |
| `fixtures/kron_pool_v2_liq_remove_base.bin` | 59126 | mainnet | 1 | 1 of 1 | `5cf12d8f7ce9be75:0` | `5f9cf6f2dffa4113` (0) |
| `fixtures/kw_vault_dms_a.bin` | 309 | testnet-10 | 1 | 1 of 1 | `a66fb82167fc7b1c:0` | `387ce62e150fc93a` (0) |
| `fixtures/kw_vault_dms_b.bin` | 309 | testnet-10 | 1 | 1 of 1 | `99d91e5b5805fc0f:0` | `fd5fdf2d8a071d0e` (0) |
| `fixtures/kw_vault_htlc_a.bin` | 371 | testnet-10 | 1 | 1 of 1 | `3ce139ca496ec95c:0` | `3cc8a9cb1847c7bf` (0) |
| `fixtures/kw_vault_htlc_b.bin` | 371 | testnet-10 | 1 | 1 of 1 | `1b26ae68c50e73d1:0` | `c4d7d9ddaef0c978` (0) |
| `fixtures/kw_vault_pqcold_a.bin` | 363 | testnet-10 | 1 | 1 of 1 | `140def6fe06f2b17:0` | `b41bfc99ce0b1da6` (0) |
| `fixtures/kw_vault_pqcold_b.bin` | 363 | testnet-10 | 1 | 1 of 1 | `d5a8891a670a25aa:0` | `6aa85c683aa3811f` (0) |
| `fixtures/kw_vault_schnorr_a.bin` | 282 | testnet-10 | 1 | 1 of 1 | `4392e3f1ffd17743:0` | `77b01905dd109ad2` (None) |
| `fixtures/kw_vault_schnorr_b.bin` | 282 | testnet-10 | 1 | 1 of 1 | `15aecf6dba545cdf:0` | `a0829e509e0352c2` (None) |
| `fixtures/log_tn_f_a.bin` | 576 | testnet-10 | 15880 | 15829 of 15829 | `13589c937d949401:0` | `3e75a99b716b7c5b` (None) |
| `fixtures/log_tn_f_b.bin` | 576 | testnet-10 | 1 | 1 of 1 | `3e75a99b716b7c5b:0` | `02a02c85d7f3c273` (None) |
| `fixtures/log_tn_f_c.bin` | 576 | testnet-10 | 15880 | 15829 of 15829 | `13589c937d949401:0` | `3e75a99b716b7c5b` (None) |
| `fixtures/pair_pool_tn_c.bin` | 6051 | testnet-10 | 4 | 3 of 3 | `52bf77bdb23ab935:0` | `ca551782a1e4bfb1` (None) |
| `fixtures/pure_a.bin` | 163 | testnet-10 | 38 | 36 of 36 | `358fc9ce06243314:0` | `dcbd036cf4be6293` (None) |
| `fixtures/pure_b.bin` | 163 | testnet-10 | 86 | 85 of 85 | `a115f2fc842a99c5:0` | `8b4057ba1afd75c6` (None) |
| `fixtures/recovery/kcc20_2728_parent_4146f7d2_0.bin` | 2728 | testnet-10 | 6 | 4 of 4 | `4146f7d298b65b35:0` | `f2d011e66aadf8ac` (0) |
| `fixtures/recovery/kcom_2671_parent_56fc521e_0.bin` | 2671 | testnet-10 | 1 | 1 of 1 | `56fc521e1ed185d3:0` | `7d5f35f096853463` (None) |
| `fixtures/recovery/kcom_8076_parent_e7da23e7_1.bin` | 8076 | testnet-10 | 1 | 1 of 1 | `e7da23e72be08353:1` | `5ca473c7eed7a145` (0) |
| `fixtures/recovery/zealous_1279_parent_5d4b4f8c_0.bin` | 1279 | testnet-10 | 1 | 1 of 1 | `5d4b4f8ca0142d90:0` | `3b71d4b73d5d6021` (0) |
| `fixtures/recovery/zealous_1363_parent_9ccf742a_0.bin` | 1363 | testnet-10 | 1 | 1 of 1 | `9ccf742aaba9f0d1:0` | `ecdd9ee7a38f45da` (0) |
| `fixtures/slot_mint_di4m_a.bin` | 370 | testnet-10 | 2 | 1 of 1 | `73bc26d8f0049e47:0` | `60f93c5f83fd3513` (None) |
| `fixtures/slot_mint_di4m_b.bin` | 370 | testnet-10 | 2 | 1 of 1 | `c673628cc62927b8:0` | `c760de1601e5bb68` (None) |
| `fixtures/slot_mint_gz4m_a.bin` | 325 | testnet-10 | 34 | 33 of 33 | `eca3ce944124df7a:0` | `82e2c9c03cec2ca1` (None) |
| `fixtures/slot_mint_gz4m_b.bin` | 325 | testnet-10 | 34 | 33 of 33 | `e58e14dc27c8453f:0` | `151782a2d1120fbb` (None) |
| `fixtures/zealous_curve_10663_a.bin` | 10663 | testnet-10 | 1 | 1 of 1 | `02325f2008380508:0` | `b46663aa3b0f7df5` (0) |
| `fixtures/zealous_curve_10663_b.bin` | 10663 | testnet-10 | 1 | 1 of 1 | `18ebb22001350c2d:0` | `1f3b08bfc993b1ae` (0) |
| `fixtures/zealous_curve_8643_a.bin` | 8643 | testnet-10 | 1 | 1 of 1 | `0e2fe36e911a4b14:0` | `150be848ca6c2d0c` (0) |
| `fixtures/zealous_curve_8643_b.bin` | 8643 | testnet-10 | 1 | 1 of 1 | `c02e5768bcd3d8c4:0` | `0becc3f009796207` (0) |
| `fixtures/zealous_curve_8802_a.bin` | 8802 | testnet-10 | 1 | 1 of 1 | `89eb99b8a312f462:0` | `4c4c291cb00f4a37` (0) |
| `fixtures/zealous_curve_8802_b.bin` | 8802 | testnet-10 | 1 | 1 of 1 | `9febaa40e8028e67:0` | `24353d2bf7b5647c` (0) |
| `fixtures/zealous_curve_8959_a.bin` | 8959 | testnet-10 | 1 | 1 of 1 | `3a27fa710e750c0a:0` | `389d000eab262b19` (0) |
| `fixtures/zealous_curve_8959_b.bin` | 8959 | testnet-10 | 1 | 1 of 1 | `df42bfc27783f80c:0` | `cf3bdf90def97a65` (0) |
| `fixtures/zealous_curve_8970_a.bin` | 8970 | testnet-10 | 1 | 1 of 1 | `9876ac1dfd7e87c8:0` | `0f7028714e1b4081` (0) |
| `fixtures/zealous_curve_8970_b.bin` | 8970 | testnet-10 | 1 | 1 of 1 | `ffa739d897caa46e:0` | `f35cd47721f5b4c2` (0) |
| `fixtures/zealous_factory_561_a.bin` | 561 | testnet-10 | 1 | 1 of 1 | `ea520f6e863b3428:0` | `7d78f0cfe6be217e` (0) |
| `fixtures/zealous_factory_561_b.bin` | 561 | testnet-10 | 1 | 1 of 1 | `7d78f0cfe6be217e:0` | `74cda7c7224d2b4b` (0) |
| `fixtures/zealous_factory_573_a.bin` | 573 | testnet-10 | 1 | 1 of 1 | `0278a81b87e66bca:0` | `47f12d11ecc2dae8` (0) |
| `fixtures/zealous_factory_573_b.bin` | 573 | testnet-10 | 1 | 1 of 1 | `47f12d11ecc2dae8:0` | `d711dfa48928ff18` (0) |
| `fixtures/zealous_factory_620_a.bin` | 620 | testnet-10 | 1 | 1 of 1 | `66c341d8569295db:0` | `d48ce828e10b2ca6` (0) |
| `fixtures/zealous_factory_620_b.bin` | 620 | testnet-10 | 1 | 1 of 1 | `7574775d48b0c43f:0` | `54d8c9ac21c71406` (0) |
| `fixtures/zealous_launch_a_a.bin` | 1279 | testnet-10 | 1 | 1 of 1 | `e5fb3710ca209f80:0` | `cf31ba4f49b9dfe8` (0) |
| `fixtures/zealous_launch_a_b.bin` | 1279 | testnet-10 | 1 | 1 of 1 | `987d30fdeef33b7e:0` | `4917d462f7f995ad` (1) |
| `fixtures/zealous_launch_b_a.bin` | 1363 | testnet-10 | 1 | 1 of 1 | `38a4c0d6cf0efb25:0` | `d88e86df4c0aff3e` (0) |
| `fixtures/zealous_launch_b_b.bin` | 1363 | testnet-10 | 2 | 2 of 2 | `c087cf4a30debe8a:0` | `15eeacc5e3a0b57e` (1) |
| `fixtures/zealous_lp_687_a.bin` | 687 | testnet-10 | 1 | 1 of 1 | `7134b04328b9079f:1` | `9c380807eefeec43` (2) |
| `fixtures/zealous_lp_687_b.bin` | 687 | testnet-10 | 1 | 1 of 1 | `78d81166317fc55a:1` | `e388b7a37acff984` (2) |
| `fixtures/zealous_lp_764_a.bin` | 764 | testnet-10 | 2 | 1 of 1 | `f2cd29057c0ae405:1` | `83c47937535e657e` (2) |
| `fixtures/zealous_lp_764_b.bin` | 764 | testnet-10 | 2 | 2 of 2 | `23927b66e080480a:1` | `59b19f2c5e60d976` (2) |
| `fixtures/zealous_lp_906_a.bin` | 906 | testnet-10 | 1 | 1 of 1 | `38ef3ac42e73b82d:1` | `9d27d80337178ec2` (2) |
| `fixtures/zealous_lp_906_b.bin` | 906 | testnet-10 | 1 | 1 of 1 | `df295528be3f7561:1` | `1d57aa73d97f7139` (2) |
| `fixtures/zealous_pool_12046_a.bin` | 12046 | testnet-10 | 1 | 1 of 1 | `d1e4d453678a7b38:0` | `4917d462f7f995ad` (0) |
| `fixtures/zealous_pool_12046_b.bin` | 12046 | testnet-10 | 1 | 1 of 1 | `c24a49de1c4b6a96:0` | `5c129c1790d8f7f9` (0) |
| `fixtures/zealous_pool_12152_a.bin` | 12152 | testnet-10 | 1 | 1 of 1 | `2cff2d0a5b5e7725:2` | `5f792c636c2b3980` (0) |
| `fixtures/zealous_pool_12152_b.bin` | 12152 | testnet-10 | 1 | 1 of 1 | `188533d50210ea74:0` | `75c4522857b0d66d` (0) |
| `fixtures/zealous_pool_12568_a.bin` | 12568 | testnet-10 | 1 | 1 of 1 | `f2cd29057c0ae405:0` | `9e420cb010a01f4d` (0) |
| `fixtures/zealous_pool_12568_b.bin` | 12568 | testnet-10 | 1 | 1 of 1 | `feab87ec34bf737c:0` | `daed938cec4335e8` (0) |
| `fixtures/zealous_pool_13478_a.bin` | 13478 | testnet-10 | 1 | 1 of 1 | `2e1e6529ebb5d4e9:0` | `2648e83666143c76` (0) |
| `fixtures/zealous_pool_13478_b.bin` | 13478 | testnet-10 | 1 | 1 of 1 | `fcac4737604ef6ec:0` | `f6f91b210f9a9a01` (0) |
| `fixtures/zealous_pool_15022_a.bin` | 15022 | testnet-10 | 1 | 1 of 1 | `ffa739d897caa46e:2` | `318b9691392c5fec` (0) |
| `fixtures/zealous_pool_15022_b.bin` | 15022 | testnet-10 | 1 | 1 of 1 | `318b9691392c5fec:0` | `782a28471399a1fc` (0) |
| `fixtures/zealous_pool_15022_c.bin` | 15022 | testnet-10 | 1 | 1 of 1 | `915825972805bb0d:0` | `a390030540cf5ac3` (0) |
| `fixtures/zealous_pool_18346_a.bin` | 18346 | testnet-10 | 1 | 1 of 1 | `df295528be3f7561:0` | `1d57aa73d97f7139` (0) |
| `fixtures/zealous_pool_18346_b.bin` | 18346 | testnet-10 | 1 | 1 of 1 | `1cf7e2460d3a22ca:0` | `2041aa57a72f221d` (0) |
| `fixtures/zealous_pool_18759_a.bin` | 18759 | testnet-10 | 1 | 1 of 1 | `537d641221c20c7d:0` | `f042b0139cd7adfa` (0) |
| `fixtures/zealous_pool_18759_b.bin` | 18759 | testnet-10 | 1 | 1 of 1 | `e69342512a395394:0` | `e9532a2dcb6ae7ce` (0) |
| `fixtures/zealous_pool_18759_genesis.bin` | 18759 | testnet-10 | 1 | 1 of 1 | `b0aa96752e7b2afd:0` | `187a4a8c1fabcf76` (0) |
| `fixtures/zealous_pool_19485_a.bin` | 19485 | testnet-10 | 1 | 1 of 1 | `641451d7ea00efef:2` | `abbfe59550668b2e` (3) |
| `fixtures/zealous_pool_19485_b.bin` | 19485 | testnet-10 | 1 | 1 of 1 | `3622c2dde53ac8aa:0` | `a8ed4de2b4ac971b` (0) |
| `fixtures/zealous_pool_19485_genesis.bin` | 19485 | testnet-10 | 1 | 1 of 1 | `1099342efd3ce14b:1` | `66086b185f2b6f8e` (0) |
| `fixtures/zealous_pool_8205_a.bin` | 8205 | testnet-10 | 1 | 1 of 1 | `eb473c32ad5c5acd:0` | `75fc336269ab05e2` (0) |
| `fixtures/zealous_pool_8205_b.bin` | 8205 | testnet-10 | 1 | 1 of 1 | `389e649232a78b97:0` | `bb44075847895292` (0) |
| `fixtures/zealous_pool_8403_a.bin` | 8403 | testnet-10 | 1 | 1 of 1 | `e8a8807bbf68fe25:0` | `cabd2046411605da` (None) |
| `fixtures/zealous_pool_8403_b.bin` | 8403 | testnet-10 | 1 | 1 of 1 | `977cbe0b669a0d95:0` | `dfa5d960b035ce76` (0) |
| `fixtures/zealous_pool_8451_a.bin` | 8451 | testnet-10 | 1 | 1 of 1 | `125ac998accf2da2:0` | `2f6b67b1736c181b` (0) |
| `fixtures/zealous_pool_8451_b.bin` | 8451 | testnet-10 | 1 | 1 of 1 | `6fe8be5302a55ad2:0` | `6144e7d3e1b993bc` (0) |
| `fixtures/zealous_pool_8622_a.bin` | 8622 | testnet-10 | 1 | 1 of 1 | `2b81bdcfefe97f38:0` | `8157b5f952531373` (0) |
| `fixtures/zealous_pool_8622_b.bin` | 8622 | testnet-10 | 1 | 1 of 1 | `debaf342ea725447:0` | `6cb16d135030c17a` (0) |
| `fixtures/zealous_position_1019_a.bin` | 1019 | testnet-10 | 3 | 1 of 1 | `e2dd1c0e1efb52b3:1` | `b46663aa3b0f7df5` (2) |
| `fixtures/zealous_position_1019_b.bin` | 1019 | testnet-10 | 1 | 1 of 1 | `18ebb22001350c2d:1` | `dc616f58f5ab2749` (2) |
| `fixtures/zealous_position_1040_a.bin` | 1040 | testnet-10 | 1 | 1 of 1 | `3a27fa710e750c0a:1` | `f4b6b515b6aea85a` (2) |
| `fixtures/zealous_position_1040_b.bin` | 1040 | testnet-10 | 4 | 4 of 4 | `9876ac1dfd7e87c8:1` | `fa1f5937c0977da1` (2) |
| `fixtures/zealous_position_980_a.bin` | 980 | testnet-10 | 1 | 1 of 1 | `0e2fe36e911a4b14:1` | `150be848ca6c2d0c` (1) |
| `fixtures/zealous_position_980_b.bin` | 980 | testnet-10 | 1 | 1 of 1 | `8032eeb5b0ea234d:1` | `12462d3d10d5d888` (2) |
| `fixtures/zealous_token_1198_a.bin` | 1198 | testnet-10 | 8 | 8 of 8 | `781fff2e60f7d03b:0` | `31253a5a0497d308` (0) |
| `fixtures/zealous_token_1198_b.bin` | 1198 | testnet-10 | 2 | 2 of 2 | `dfdc7383cd32ae96:0` | `389e649232a78b97` (1) |
| `fixtures/zealous_token_1324_a.bin` | 1324 | testnet-10 | 2 | 2 of 2 | `7fc94eeac6429041:0` | `ec8eb076ad88d72d` (0) |
| `fixtures/zealous_token_1324_b.bin` | 1324 | testnet-10 | 1 | 1 of 1 | `ec8eb076ad88d72d:0` | `9e420cb010a01f4d` (1) |
| `fixtures/zealous_token_1811_a.bin` | 1811 | testnet-10 | 8 | 8 of 8 | `c2e8d857c96829ca:0` | `ac330c7ab547a5c2` (None) |
| `fixtures/zealous_token_1811_b.bin` | 1811 | testnet-10 | 2 | 2 of 2 | `7afc6c6563413968:0` | `260fb02c56df11b0` (None) |
| `tests/fixtures/argent-zealous-cell-573.hex` | 573 | testnet-10 | 1 | 1 of 1 | `0278a81b87e66bca:0` | `47f12d11ecc2dae8` (0) |
| `tests/fixtures/argent-zealous-order-cell.hex` | 980 | testnet-10 | 1 | 1 of 1 | `0e2fe36e911a4b14:1` | `150be848ca6c2d0c` (1) |
| `tests/fixtures/argent-zealous-token-cell.hex` | 1279 | testnet-10 | 1 | 1 of 1 | `e5fb3710ca209f80:0` | `cf31ba4f49b9dfe8` (0) |

## Signature scripts found on chain

| File | Bytes | Network | Spends | In transaction (input) |
|---|---|---|---|---|
| `fixtures/kron_pool_v2_liq_add_witness.bin` | 59260 | mainnet | `66ad481faae88acb:0` | `cdd39382aa7c7e78` (0) |
| `fixtures/kron_pool_v2_liq_remove_witness.bin` | 59262 | mainnet | `5cf12d8f7ce9be75:0` | `5f9cf6f2dffa4113` (0) |
| `fixtures/recovery/kcc20_foreign_witness_0dd4fbe2_3.bin` | 3230 | testnet-10 | `0dd4fbe2e74309ff:3` | `9e629f204b6a68a3` (2) |
| `fixtures/recovery/kcc20_witness_b438a262.bin` | 1728 | testnet-10 | `aacd228491b59c19:0` | `b438a26252eacf2c` (None) |
| `fixtures/recovery/kcom_witness_5ca473c7.bin` | 8125 | testnet-10 | `e7da23e72be08353:1` | `5ca473c7eed7a145` (0) |
| `fixtures/recovery/kcom_witness_7d5f35f0.bin` | 2718 | testnet-10 | `56fc521e1ed185d3:0` | `7d5f35f096853463` (None) |
| `fixtures/recovery/zealous_witness_3b71d4b7.bin` | 1423 | testnet-10 | `5d4b4f8ca0142d90:0` | `3b71d4b73d5d6021` (0) |
| `fixtures/recovery/zealous_witness_ecdd9ee7.bin` | 1502 | testnet-10 | `9ccf742aaba9f0d1:0` | `ecdd9ee7a38f45da` (0) |
| `fixtures/recovery/zealous_witness_f296e570.bin` | 1955 | testnet-10 | `c0c67773dbd06dc8:0` | `f296e570fae81a1a` (0) |

## Not a whole on-chain output

| File | Bytes | Origin |
|---|---|---|
| `fixtures/kcc0020/vector_template_a.bin` | 2384 | kascov's own reduced build of the KCC-0020 draft (kaspanet/kccs at ea5176aa). Built, not taken from chain. |
| `fixtures/kcc0020/vector_template_b.bin` | 2384 | kascov's own reduced build of the KCC-0020 draft (kaspanet/kccs at ea5176aa), with different arguments. Built, not taken from chain. |
| `fixtures/kcc20_kron_launch_args.bin` | 78 | The argument pushes of input 0's signature script in transaction c3faf69d..., with the 172 KB program push that follows cut off to keep the fixture small. A fragment of a witness. |
| `fixtures/kcm_curve_v1.bin` | 17269 | A curve build compiled from its builder's published source with the builder's pinned compiler. Checkable by compiling that source, not by a chain reveal. |
| `fixtures/kcm_curve_v1_variant.bin` | 17269 | The same curve source compiled with different arguments, to find which pushes vary. |
| `fixtures/kcm_order_live_tn10.bin` | 4531 | A real order posted to testnet-10 and filled there, recovered from its reveal. kascov's current index holds no output that commits to it as plain P2SH, so no outpoint is listed. |
| `fixtures/kcm_order_v1.bin` | 4531 | A resting limit order compiled from its builder's published source with the pinned compiler. Checkable by compiling that source. |
| `fixtures/kcm_order_v1_variant.bin` | 4531 | The same order source compiled with a different offer, to find which pushes vary. |
| `fixtures/kron_pool_tn_a.bin` | 55265 | KRON's oldest pool template, live only on testnet-10. Its ten deployments, recovered from their own spends, match it outside four creator slots, so no single output commits to exactly these bytes. |
| `fixtures/kron_pool_tn_e.bin` | 59089 | The pool template KRON's testnet curve tn-e graduates into, from a real testnet-10 reveal. A template: real pools match it outside their creator slots. |
| `fixtures/kron_pool_v1.bin` | 57475 | A KRON pool template, taken from the copy embedded in a hash-proven KRON curve program. Deployed pools differ from it only in their instance slots, so no single output commits to exactly these bytes. |
| `fixtures/kron_pool_v2.bin` | 59032 | The second-generation KRON pool template, taken from the copy embedded in a hash-proven KRON curve program. Deployed pools differ from it only in their instance slots. |
| `fixtures/kron_pool_v3.bin` | 59089 | The v3 KRON pool template, taken from the copy embedded (twice, byte-identically) in the hash-proven v3 curve program. The first v3-graduated mainnet pool matches it outside its two creator slots. |
| `fixtures/vesting_kron_prefix.bin` | 1 | The prefix of KRON's vesting template, in front of its 69-byte state region (see src/vesting.rs). A fragment, so no output commits to it. |
| `fixtures/vesting_kron_suffix.bin` | 18177 | The code of KRON's vesting template after its 69-byte state region (see src/vesting.rs). A fragment, so no output commits to it. |
| `tests/fixtures/argent-order-cell-suffix.hex` | 905 | The code half of a program revealed on testnet-10 by a spend of covenant 35d0f746e66010eedadad11e... (see tests/argent_template.rs). A fragment, so no output commits to it. |
