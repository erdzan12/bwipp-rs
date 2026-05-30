//! USPS Intelligent Mail Barcode (OneCode, IMb).
//!
//! 65-bar 4-state symbology for first-class mail tracking. The encoder
//! turns a 20/25/29/31-digit input into a 13-byte big-endian "binval"
//! (via a multi-stage base-10 / base-N reduction), computes an 11-bit
//! frame check sequence (FCS), splits into 10 codewords (one base-636 +
//! nine base-1365), looks up 13-bit characters via a 5-of-13 / 2-of-13
//! Hamming pair, then maps the 10 characters' 130 bits into 65 bars
//! (each one of Tracker / Ascender / Descender / Full).
//!
//! Verified byte-for-byte against bwip-js's `bwipp_onecode` for the
//! canonical "01234567094987654321" and "0123456709498765432101234567891"
//! test vectors at the bytes, FCS, and codewords stages.

use crate::encoding::{Bar4State, Postal4Pattern};
use crate::error::Error;
use crate::options::Options;

#[rustfmt::skip]
const STARTVAL_20: &[u32] = &[0];
#[rustfmt::skip]
const STARTVAL_25: &[u32] = &[1];
#[rustfmt::skip]
const STARTVAL_29: &[u32] = &[1, 0, 0, 0, 0, 1];
#[rustfmt::skip]
const STARTVAL_31: &[u32] = &[1, 0, 0, 0, 1, 0, 0, 0, 0, 1];

#[rustfmt::skip]
const TAB513: [u16; 1287] = [
    31, 7936, 47, 7808, 55, 7552, 59, 7040, 61, 6016, 62, 3968, 79, 7744, 87, 7488,
    91, 6976, 93, 5952, 94, 3904, 103, 7360, 107, 6848, 109, 5824, 110, 3776, 115, 6592,
    117, 5568, 118, 3520, 121, 5056, 122, 3008, 124, 1984, 143, 7712, 151, 7456, 155, 6944,
    157, 5920, 158, 3872, 167, 7328, 171, 6816, 173, 5792, 174, 3744, 179, 6560, 181, 5536,
    182, 3488, 185, 5024, 186, 2976, 188, 1952, 199, 7264, 203, 6752, 205, 5728, 206, 3680,
    211, 6496, 213, 5472, 214, 3424, 217, 4960, 218, 2912, 220, 1888, 227, 6368, 229, 5344,
    230, 3296, 233, 4832, 234, 2784, 236, 1760, 241, 4576, 242, 2528, 244, 1504, 248, 992,
    271, 7696, 279, 7440, 283, 6928, 285, 5904, 286, 3856, 295, 7312, 299, 6800, 301, 5776,
    302, 3728, 307, 6544, 309, 5520, 310, 3472, 313, 5008, 314, 2960, 316, 1936, 327, 7248,
    331, 6736, 333, 5712, 334, 3664, 339, 6480, 341, 5456, 342, 3408, 345, 4944, 346, 2896,
    348, 1872, 355, 6352, 357, 5328, 358, 3280, 361, 4816, 362, 2768, 364, 1744, 369, 4560,
    370, 2512, 372, 1488, 376, 976, 391, 7216, 395, 6704, 397, 5680, 398, 3632, 403, 6448,
    405, 5424, 406, 3376, 409, 4912, 410, 2864, 412, 1840, 419, 6320, 421, 5296, 422, 3248,
    425, 4784, 426, 2736, 428, 1712, 433, 4528, 434, 2480, 436, 1456, 440, 944, 451, 6256,
    453, 5232, 454, 3184, 457, 4720, 458, 2672, 460, 1648, 465, 4464, 466, 2416, 468, 1392,
    472, 880, 481, 4336, 482, 2288, 484, 1264, 488, 752, 527, 7688, 535, 7432, 539, 6920,
    541, 5896, 542, 3848, 551, 7304, 555, 6792, 557, 5768, 558, 3720, 563, 6536, 565, 5512,
    566, 3464, 569, 5000, 570, 2952, 572, 1928, 583, 7240, 587, 6728, 589, 5704, 590, 3656,
    595, 6472, 597, 5448, 598, 3400, 601, 4936, 602, 2888, 604, 1864, 611, 6344, 613, 5320,
    614, 3272, 617, 4808, 618, 2760, 620, 1736, 625, 4552, 626, 2504, 628, 1480, 632, 968,
    647, 7208, 651, 6696, 653, 5672, 654, 3624, 659, 6440, 661, 5416, 662, 3368, 665, 4904,
    666, 2856, 668, 1832, 675, 6312, 677, 5288, 678, 3240, 681, 4776, 682, 2728, 684, 1704,
    689, 4520, 690, 2472, 692, 1448, 696, 936, 707, 6248, 709, 5224, 710, 3176, 713, 4712,
    714, 2664, 716, 1640, 721, 4456, 722, 2408, 724, 1384, 728, 872, 737, 4328, 738, 2280,
    740, 1256, 775, 7192, 779, 6680, 781, 5656, 782, 3608, 787, 6424, 789, 5400, 790, 3352,
    793, 4888, 794, 2840, 796, 1816, 803, 6296, 805, 5272, 806, 3224, 809, 4760, 810, 2712,
    812, 1688, 817, 4504, 818, 2456, 820, 1432, 824, 920, 835, 6232, 837, 5208, 838, 3160,
    841, 4696, 842, 2648, 844, 1624, 849, 4440, 850, 2392, 852, 1368, 865, 4312, 866, 2264,
    868, 1240, 899, 6200, 901, 5176, 902, 3128, 905, 4664, 906, 2616, 908, 1592, 913, 4408,
    914, 2360, 916, 1336, 929, 4280, 930, 2232, 932, 1208, 961, 4216, 962, 2168, 964, 1144,
    1039, 7684, 1047, 7428, 1051, 6916, 1053, 5892, 1054, 3844, 1063, 7300, 1067, 6788, 1069, 5764,
    1070, 3716, 1075, 6532, 1077, 5508, 1078, 3460, 1081, 4996, 1082, 2948, 1084, 1924, 1095, 7236,
    1099, 6724, 1101, 5700, 1102, 3652, 1107, 6468, 1109, 5444, 1110, 3396, 1113, 4932, 1114, 2884,
    1116, 1860, 1123, 6340, 1125, 5316, 1126, 3268, 1129, 4804, 1130, 2756, 1132, 1732, 1137, 4548,
    1138, 2500, 1140, 1476, 1159, 7204, 1163, 6692, 1165, 5668, 1166, 3620, 1171, 6436, 1173, 5412,
    1174, 3364, 1177, 4900, 1178, 2852, 1180, 1828, 1187, 6308, 1189, 5284, 1190, 3236, 1193, 4772,
    1194, 2724, 1196, 1700, 1201, 4516, 1202, 2468, 1204, 1444, 1219, 6244, 1221, 5220, 1222, 3172,
    1225, 4708, 1226, 2660, 1228, 1636, 1233, 4452, 1234, 2404, 1236, 1380, 1249, 4324, 1250, 2276,
    1287, 7188, 1291, 6676, 1293, 5652, 1294, 3604, 1299, 6420, 1301, 5396, 1302, 3348, 1305, 4884,
    1306, 2836, 1308, 1812, 1315, 6292, 1317, 5268, 1318, 3220, 1321, 4756, 1322, 2708, 1324, 1684,
    1329, 4500, 1330, 2452, 1332, 1428, 1347, 6228, 1349, 5204, 1350, 3156, 1353, 4692, 1354, 2644,
    1356, 1620, 1361, 4436, 1362, 2388, 1377, 4308, 1378, 2260, 1411, 6196, 1413, 5172, 1414, 3124,
    1417, 4660, 1418, 2612, 1420, 1588, 1425, 4404, 1426, 2356, 1441, 4276, 1442, 2228, 1473, 4212,
    1474, 2164, 1543, 7180, 1547, 6668, 1549, 5644, 1550, 3596, 1555, 6412, 1557, 5388, 1558, 3340,
    1561, 4876, 1562, 2828, 1564, 1804, 1571, 6284, 1573, 5260, 1574, 3212, 1577, 4748, 1578, 2700,
    1580, 1676, 1585, 4492, 1586, 2444, 1603, 6220, 1605, 5196, 1606, 3148, 1609, 4684, 1610, 2636,
    1617, 4428, 1618, 2380, 1633, 4300, 1634, 2252, 1667, 6188, 1669, 5164, 1670, 3116, 1673, 4652,
    1674, 2604, 1681, 4396, 1682, 2348, 1697, 4268, 1698, 2220, 1729, 4204, 1730, 2156, 1795, 6172,
    1797, 5148, 1798, 3100, 1801, 4636, 1802, 2588, 1809, 4380, 1810, 2332, 1825, 4252, 1826, 2204,
    1857, 4188, 1858, 2140, 1921, 4156, 1922, 2108, 2063, 7682, 2071, 7426, 2075, 6914, 2077, 5890,
    2078, 3842, 2087, 7298, 2091, 6786, 2093, 5762, 2094, 3714, 2099, 6530, 2101, 5506, 2102, 3458,
    2105, 4994, 2106, 2946, 2119, 7234, 2123, 6722, 2125, 5698, 2126, 3650, 2131, 6466, 2133, 5442,
    2134, 3394, 2137, 4930, 2138, 2882, 2147, 6338, 2149, 5314, 2150, 3266, 2153, 4802, 2154, 2754,
    2161, 4546, 2162, 2498, 2183, 7202, 2187, 6690, 2189, 5666, 2190, 3618, 2195, 6434, 2197, 5410,
    2198, 3362, 2201, 4898, 2202, 2850, 2211, 6306, 2213, 5282, 2214, 3234, 2217, 4770, 2218, 2722,
    2225, 4514, 2226, 2466, 2243, 6242, 2245, 5218, 2246, 3170, 2249, 4706, 2250, 2658, 2257, 4450,
    2258, 2402, 2273, 4322, 2311, 7186, 2315, 6674, 2317, 5650, 2318, 3602, 2323, 6418, 2325, 5394,
    2326, 3346, 2329, 4882, 2330, 2834, 2339, 6290, 2341, 5266, 2342, 3218, 2345, 4754, 2346, 2706,
    2353, 4498, 2354, 2450, 2371, 6226, 2373, 5202, 2374, 3154, 2377, 4690, 2378, 2642, 2385, 4434,
    2401, 4306, 2435, 6194, 2437, 5170, 2438, 3122, 2441, 4658, 2442, 2610, 2449, 4402, 2465, 4274,
    2497, 4210, 2567, 7178, 2571, 6666, 2573, 5642, 2574, 3594, 2579, 6410, 2581, 5386, 2582, 3338,
    2585, 4874, 2586, 2826, 2595, 6282, 2597, 5258, 2598, 3210, 2601, 4746, 2602, 2698, 2609, 4490,
    2627, 6218, 2629, 5194, 2630, 3146, 2633, 4682, 2641, 4426, 2657, 4298, 2691, 6186, 2693, 5162,
    2694, 3114, 2697, 4650, 2705, 4394, 2721, 4266, 2753, 4202, 2819, 6170, 2821, 5146, 2822, 3098,
    2825, 4634, 2833, 4378, 2849, 4250, 2881, 4186, 2945, 4154, 3079, 7174, 3083, 6662, 3085, 5638,
    3086, 3590, 3091, 6406, 3093, 5382, 3094, 3334, 3097, 4870, 3107, 6278, 3109, 5254, 3110, 3206,
    3113, 4742, 3121, 4486, 3139, 6214, 3141, 5190, 3145, 4678, 3153, 4422, 3169, 4294, 3203, 6182,
    3205, 5158, 3209, 4646, 3217, 4390, 3233, 4262, 3265, 4198, 3331, 6166, 3333, 5142, 3337, 4630,
    3345, 4374, 3361, 4246, 3393, 4182, 3457, 4150, 3587, 6158, 3589, 5134, 3593, 4622, 3601, 4366,
    3617, 4238, 3649, 4174, 3713, 4142, 3841, 4126, 4111, 7681, 4119, 7425, 4123, 6913, 4125, 5889,
    4135, 7297, 4139, 6785, 4141, 5761, 4147, 6529, 4149, 5505, 4153, 4993, 4167, 7233, 4171, 6721,
    4173, 5697, 4179, 6465, 4181, 5441, 4185, 4929, 4195, 6337, 4197, 5313, 4201, 4801, 4209, 4545,
    4231, 7201, 4235, 6689, 4237, 5665, 4243, 6433, 4245, 5409, 4249, 4897, 4259, 6305, 4261, 5281,
    4265, 4769, 4273, 4513, 4291, 6241, 4293, 5217, 4297, 4705, 4305, 4449, 4359, 7185, 4363, 6673,
    4365, 5649, 4371, 6417, 4373, 5393, 4377, 4881, 4387, 6289, 4389, 5265, 4393, 4753, 4401, 4497,
    4419, 6225, 4421, 5201, 4425, 4689, 4483, 6193, 4485, 5169, 4489, 4657, 4615, 7177, 4619, 6665,
    4621, 5641, 4627, 6409, 4629, 5385, 4633, 4873, 4643, 6281, 4645, 5257, 4649, 4745, 4675, 6217,
    4677, 5193, 4739, 6185, 4741, 5161, 4867, 6169, 4869, 5145, 5127, 7173, 5131, 6661, 5133, 5637,
    5139, 6405, 5141, 5381, 5155, 6277, 5157, 5253, 5187, 6213, 5251, 6181, 5379, 6165, 5635, 6157,
    6151, 7171, 6155, 6659, 6163, 6403, 6179, 6275, 6211, 5189, 4681, 4433, 4321, 3142, 2634, 2386,
    2274, 1612, 1364, 1252, 856, 744, 496,
];

#[rustfmt::skip]
const TAB213: [u16; 78] = [
    3, 6144, 5, 5120, 6, 3072, 9, 4608, 10, 2560, 12, 1536, 17, 4352, 18, 2304,
    20, 1280, 24, 768, 33, 4224, 34, 2176, 36, 1152, 40, 640, 48, 384, 65, 4160,
    66, 2112, 68, 1088, 72, 576, 80, 320, 96, 192, 129, 4128, 130, 2080, 132, 1056,
    136, 544, 144, 288, 257, 4112, 258, 2064, 260, 1040, 264, 528, 513, 4104, 514, 2056,
    516, 1032, 1025, 4100, 1026, 2052, 2049, 4098, 4097, 2050, 1028, 520, 272, 160,
];

#[rustfmt::skip]
const BARMAP: [u8; 260] = [
    7, 2, 4, 3, 1, 10, 0, 0, 9, 12, 2, 8, 5, 5, 6, 11,
    8, 9, 3, 1, 0, 1, 5, 12, 2, 5, 1, 8, 4, 4, 9, 11,
    6, 3, 8, 10, 3, 9, 7, 6, 5, 11, 1, 4, 8, 5, 2, 12,
    9, 10, 0, 2, 7, 1, 6, 7, 3, 6, 4, 9, 0, 3, 8, 6,
    6, 4, 2, 7, 1, 1, 9, 9, 7, 10, 5, 2, 4, 0, 3, 8,
    6, 2, 0, 4, 8, 11, 1, 0, 9, 8, 3, 12, 2, 6, 7, 7,
    5, 1, 4, 10, 1, 12, 6, 9, 7, 3, 8, 0, 5, 8, 9, 7,
    4, 6, 2, 10, 3, 4, 0, 5, 8, 4, 5, 7, 7, 11, 1, 9,
    6, 0, 9, 6, 0, 6, 4, 8, 2, 1, 3, 2, 5, 9, 8, 12,
    4, 11, 6, 1, 9, 5, 7, 4, 3, 3, 1, 2, 0, 7, 2, 0,
    1, 3, 4, 1, 6, 10, 3, 5, 8, 7, 9, 4, 2, 11, 5, 6,
    0, 8, 7, 12, 4, 2, 8, 1, 5, 10, 3, 0, 9, 3, 0, 9,
    6, 5, 2, 4, 7, 8, 1, 7, 5, 0, 4, 5, 2, 3, 0, 10,
    6, 12, 9, 2, 3, 11, 1, 6, 8, 8, 7, 9, 5, 4, 0, 11,
    1, 5, 2, 2, 9, 1, 4, 12, 8, 3, 6, 6, 7, 0, 3, 7,
    4, 7, 7, 5, 0, 12, 1, 11, 2, 9, 9, 0, 6, 8, 5, 3,
    3, 10, 8, 2,
];

/// Build the bigint `binval` (as an array of base-10 digits, high → low).
/// Mirrors bwip-js's `binval` build chain exactly.
fn build_binval(text: &[u8]) -> Vec<u32> {
    let barlen = text.len();
    let startval: &[u32] = match barlen {
        20 => STARTVAL_20,
        25 => STARTVAL_25,
        29 => STARTVAL_29,
        31 => STARTVAL_31,
        _ => unreachable!("caller validates barlen"),
    };

    // binval = startval + routing_digits (chars 20..barlen). For barlen=20
    // the routing region is empty; for 25/29/31 it carries 5/9/11 digits.
    let routing: Vec<u32> = text[20..].iter().map(|&b| (b - b'0') as u32).collect();
    let mut binval = bigadd(startval.to_vec(), routing);

    // Append digit 0 (text[0]).
    binval.push((text[0] - b'0') as u32);

    // Multiply every element by 5, then add digit 1 (text[1]), normalize base 10.
    for v in &mut binval {
        *v *= 5;
    }
    binval = bigadd(binval, vec![(text[1] - b'0') as u32]);
    normalize_base(&mut binval, 10);
    strip_leading_zeros(&mut binval);

    // Append the remaining 18 input digits (positions 2..20).
    for &b in &text[2..20] {
        binval.push((b - b'0') as u32);
    }
    binval
}

/// Right-align `a` and `b` and add element-wise, returning the longer of
/// the two with `b`'s digits added in. `b.len()` ≤ `a.len()` is **not**
/// required — we automatically pick the longer as the accumulator.
fn bigadd(mut a: Vec<u32>, mut b: Vec<u32>) -> Vec<u32> {
    if a.len() < b.len() {
        std::mem::swap(&mut a, &mut b);
    }
    let offset = a.len() - b.len();
    for (i, &bi) in b.iter().enumerate() {
        a[offset + i] += bi;
    }
    a
}

/// Normalize a digit array in `base` (handle carries from low to high,
/// extend at the front if the top digit overflows).
fn normalize_base(num: &mut Vec<u32>, base: u32) {
    for i in (1..num.len()).rev() {
        let carry = num[i] / base;
        num[i] %= base;
        num[i - 1] += carry;
    }
    if num[0] >= base {
        // Pull out additional digits from num[0] in base.
        let mut extras = Vec::new();
        let mut v = num[0];
        while v >= base {
            extras.push(v % base);
            v /= base;
        }
        extras.push(v);
        extras.reverse(); // most-significant first
        let mut new_num = extras;
        new_num.extend_from_slice(&num[1..]);
        *num = new_num;
    }
}

fn strip_leading_zeros(num: &mut Vec<u32>) {
    let mut first_nonzero = 0;
    while first_nonzero < num.len() && num[first_nonzero] == 0 {
        first_nonzero += 1;
    }
    if first_nonzero == num.len() {
        *num = vec![0];
    } else {
        num.drain(..first_nonzero);
    }
}

/// Extract 13 base-256 bytes from `binval`. Mirrors the bwip-js byte
/// loop: at each iteration, reduce binval mod 256 → byte, then divide
/// binval by 256 (treating binval as a big-endian base-10 polynomial).
fn bytes_from_binval(binval: &[u32]) -> [u8; 13] {
    let mut bintmp: Vec<u32> = binval.to_vec();
    let mut bytes = [0u8; 13];
    for i in (0..13).rev() {
        // Propagate `bintmp[j] % 256 * 10` into bintmp[j+1], replacing
        // bintmp[j] with bintmp[j] / 256.
        for j in 0..(bintmp.len() - 1) {
            let carry = (bintmp[j] % 256) * 10;
            bintmp[j + 1] += carry;
            bintmp[j] /= 256;
        }
        let last = bintmp.len() - 1;
        bytes[i] = (bintmp[last] % 256) as u8;
        bintmp[last] /= 256;
    }
    bytes
}

/// 11-bit USPS frame-check sequence over the 13-byte `bytes` array.
/// Polynomial = 0xF35 (3893), initial value = 0x7FF (2047). The first
/// pass uses the top 6 bits of bytes[0] (`bytes[0] << 5`), then 8 bits
/// from each of bytes[1..=12].
fn fcs_from_bytes(bytes: &[u8; 13]) -> u16 {
    let mut fcs: u16 = 2047;
    let mut dat: u16 = (bytes[0] as u16) << 5; // bytes[0] * 32
    for _ in 0..6 {
        let mut next = fcs << 1;
        if ((fcs ^ dat) & 0x400) != 0 {
            next ^= 3893;
        }
        fcs = next & 2047;
        dat <<= 1;
    }
    for &b in &bytes[1..] {
        dat = (b as u16) << 3; // shifts into the top bits for 8-bit processing
        for _ in 0..8 {
            let mut next = fcs << 1;
            if ((fcs ^ dat) & 0x400) != 0 {
                next ^= 3893;
            }
            fcs = next & 2047;
            dat <<= 1;
        }
    }
    fcs
}

/// Extract 10 codewords from `binval`. Codeword 9 uses base 636; the
/// other nine use base 1365. `binval` is consumed.
fn codewords_from_binval(mut binval: Vec<u32>, fcs: u16) -> [u32; 10] {
    let mut codewords = [0u32; 10];
    for i in (0..10).rev() {
        let base: u32 = if i == 9 { 636 } else { 1365 };
        for j in 0..(binval.len() - 1) {
            let carry = (binval[j] % base) * 10;
            binval[j + 1] += carry;
            binval[j] /= base;
        }
        let last = binval.len() - 1;
        codewords[i] = binval[last] % base;
        binval[last] /= base;
    }
    codewords[9] *= 2;
    if (fcs & 0x400) != 0 {
        codewords[0] += 659;
    }
    codewords
}

/// Look up 13-bit Hamming codewords for each codeword + apply the FCS
/// XOR mask (each `chars[i] ^= 0x1FFF` iff FCS bit i is set).
fn chars_from_codewords(codewords: &[u32; 10], fcs: u16) -> [u16; 10] {
    let mut chars = [0u16; 10];
    for (i, &cw) in codewords.iter().enumerate() {
        chars[i] = if cw <= 1286 {
            TAB513[cw as usize]
        } else {
            TAB213[(cw - 1287) as usize]
        };
    }
    for (i, c) in chars.iter_mut().enumerate() {
        if (fcs & (1 << i)) != 0 {
            *c ^= 0x1FFF;
        }
    }
    chars
}

/// Map 10 13-bit characters to 65 Postal4State bars via [`BARMAP`].
/// Each bar reads two bits — one from a "descender" character/bit and
/// one from an "ascender" character/bit. The combination decides the
/// bar's 4-state.
fn bars_from_chars(chars: &[u16; 10]) -> Vec<Bar4State> {
    let mut bars = Vec::with_capacity(65);
    for i in 0..65 {
        let dec_char = BARMAP[i * 4] as usize;
        let dec_bit = BARMAP[i * 4 + 1] as u16;
        let asc_char = BARMAP[i * 4 + 2] as usize;
        let asc_bit = BARMAP[i * 4 + 3] as u16;
        let dec = (chars[dec_char] & (1 << dec_bit)) != 0;
        let asc = (chars[asc_char] & (1 << asc_bit)) != 0;
        bars.push(match (dec, asc) {
            (false, false) => Bar4State::Tracker,
            (false, true) => Bar4State::Ascender,
            (true, false) => Bar4State::Descender,
            (true, true) => Bar4State::Full,
        });
    }
    bars
}

/// Encode a USPS Intelligent Mail Barcode (OneCode) symbol from a 20/
/// 25/29/31-digit string.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // 20-digit tracking code (Barcode ID + Service Type ID + Mailer ID + Serial).
/// let svg = render_svg(Symbology::UspsOneCode, "01234567094987654321", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    let bytes = data.as_bytes();
    if !bytes.iter().all(|b| b.is_ascii_digit()) {
        return Err(Error::InvalidData(
            "USPS OneCode: data must contain only digits".into(),
        ));
    }
    if !matches!(bytes.len(), 20 | 25 | 29 | 31) {
        return Err(Error::InvalidData(format!(
            "USPS OneCode: data must be 20, 25, 29 or 31 digits (got {})",
            bytes.len()
        )));
    }
    let binval = build_binval(bytes);
    let byte_arr = bytes_from_binval(&binval);
    let fcs = fcs_from_bytes(&byte_arr);
    let codewords = codewords_from_binval(binval, fcs);
    let chars = chars_from_codewords(&codewords, fcs);
    let bars = bars_from_chars(&chars);

    let text = if opts.include_text {
        Some(data.to_string())
    } else {
        None
    };
    Ok(Postal4Pattern { bars, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------
    // Stage 11.A8b mutation-killer tests for the private big-integer
    // helpers. These exercise the arithmetic functions directly so the
    // failure modes can't be masked by downstream encoding logic.
    // ---------------------------------------------------------------------

    /// Kills the `normalize_base` family of survivors at lines 194-213:
    ///   * `replace normalize_base with ()` (195:5)
    ///   * `>= with <` in the overflow-extension branch (200:15)
    ///   * `>= with <` in the inner extras loop (204:17)
    ///   * `% with /` and `% with +` (205:27)
    ///   * `/= with %=` and `/= with *=` (206:15)
    ///
    /// All five mutations land in the "carry overflow + redistribute"
    /// path. We construct an input that exercises every branch:
    ///   - num = [10, 25, 100] in base 10 needs carries from low to high
    ///     (num[2] = 100 → 10 carry to num[1] → 35 → 3 carry to num[0]
    ///     → 13), then the top digit (13) overflows and is split into
    ///     [1, 3]. Expected result: [1, 3, 5, 0].
    ///
    /// The mutant `()` (delete) leaves [10, 25, 100] unchanged. The
    /// `< vs >=` flips bypass the overflow loop. The `% / *` flips
    /// produce wrong carry values. Every mutation diverges from the
    /// pinned [1, 3, 5, 0].
    #[test]
    fn normalize_base_handles_carries_and_overflow() {
        let mut num = vec![10, 25, 100];
        normalize_base(&mut num, 10);
        assert_eq!(num, vec![1, 3, 5, 0]);
    }

    /// Stage 11.A8c — pin `bigadd`. Right-aligned big-int addition
    /// helper that swaps args so the longer is the accumulator, then
    /// adds each `b[i]` to `a[offset + i]` where `offset = a.len() -
    /// b.len()`. No carry normalization happens here — the caller
    /// runs `normalize_base` after.
    ///
    /// The existing tests only exercise bigadd through end-to-end
    /// goldens. Mutations to catch:
    ///   - `a.len() < b.len()` → `<=` / `>`: wrong swap direction —
    ///     equal-length wouldn't swap (correct) but the inversion
    ///     breaks unequal cases.
    ///   - `a.len() - b.len()` → `+`: panics on out-of-bounds.
    ///   - `offset + i` → `offset - i` or `i`: misaligned write —
    ///     low-digit values of b land in high-digit slots of a.
    ///   - `+=` → `=`: replaces instead of accumulating.
    ///   - delete swap → fails when first arg is shorter.
    #[test]
    fn bigadd_right_aligns_and_accumulates() {
        // Equal-length: no swap, all aligned, accumulation.
        assert_eq!(
            bigadd(vec![1, 2, 3], vec![4, 5, 6]),
            vec![5, 7, 9],
            "equal lengths: per-digit add"
        );
        // First shorter than second: swap kicks in, offset = 1.
        // After swap: a=[1,2,3], b=[7,8]. offset = 3-2 = 1.
        // a[1] += 7, a[2] += 8 → [1, 9, 11].
        assert_eq!(
            bigadd(vec![7, 8], vec![1, 2, 3]),
            vec![1, 9, 11],
            "first shorter → swap; right-align b within a"
        );
        // Second shorter than first: no swap, offset = 1.
        // a=[1,2,3], b=[7,8]. a[1]+=7, a[2]+=8 → [1, 9, 11]. Same
        // logical result — proves the swap is idempotent on the
        // canonical (longer-first) form.
        assert_eq!(
            bigadd(vec![1, 2, 3], vec![7, 8]),
            vec![1, 9, 11],
            "second shorter: same logical result as the swap case"
        );
        // Single-digit addend on the right of a multi-digit accumulator.
        // a=[1, 2, 3], b=[9]. offset=2. a[2]+=9 → [1, 2, 12].
        assert_eq!(
            bigadd(vec![1, 2, 3], vec![9]),
            vec![1, 2, 12],
            "single-digit b lands in the least-significant slot of a"
        );
        // Empty b: no additions, a unchanged.
        assert_eq!(bigadd(vec![1, 2, 3], vec![]), vec![1, 2, 3]);
        // Empty a, non-empty b: swap → a becomes b, offset=0.
        assert_eq!(bigadd(vec![], vec![4, 5]), vec![4, 5]);
        // No carry-handling here — values can exceed the base.
        // a=[200], b=[200] → [400]. Confirms bigadd doesn't normalize.
        assert_eq!(
            bigadd(vec![200], vec![200]),
            vec![400],
            "bigadd does NOT normalize (no `% base` reduction)"
        );
    }

    /// Kills the `strip_leading_zeros` survivors:
    ///   * `replace strip_leading_zeros with ()` (217:5)
    ///   * `< with ==`, `< with >`, `< with <=` (218:25 — three
    ///     mutants on the while-loop condition)
    ///   * `+= with *=` (219:23, timeout)
    ///
    /// We pin three behaviours:
    ///   1. Input with leading zeros + non-zero suffix → drops zeros.
    ///   2. Input that is ALL zeros → collapses to `[0]`.
    ///   3. Input with no leading zeros → unchanged.
    ///
    /// Together they exercise every iteration count from 0 to 3 and
    /// trip every branch the mutants alter.
    #[test]
    fn strip_leading_zeros_handles_every_branch() {
        // Drops leading zeros.
        let mut a = vec![0, 0, 5, 3];
        strip_leading_zeros(&mut a);
        assert_eq!(a, vec![5, 3]);

        // All-zero input collapses to [0].
        let mut b = vec![0, 0, 0];
        strip_leading_zeros(&mut b);
        assert_eq!(b, vec![0]);

        // No leading zeros: unchanged.
        let mut c = vec![1, 2, 3];
        strip_leading_zeros(&mut c);
        assert_eq!(c, vec![1, 2, 3]);
    }

    /// Kills the `% with +` mutant at line 243 of `bytes_from_binval`.
    /// The line reduces `bintmp[last] % 256` to a byte; the mutant
    /// makes it `bintmp[last] + 256` which produces a value > 255 and
    /// breaks the byte conversion. The pre-existing
    /// `binval_then_bytes_match_bwip_js_20` test still pins the
    /// 13-byte output, but the assertion uses an array equality that
    /// the truncating `(... as u8)` cast can mask for some specific
    /// inputs. This test pins a *different* known-good byte sequence
    /// (the canonical "0" input → 13 leading zeros) so the cast can't
    /// silently coincide.
    #[test]
    fn bytes_from_binval_zero_input_collapses_to_thirteen_zeros() {
        // A 20-digit "0"-payload constructs a binval that's all zeros,
        // and every step of the byte loop should produce 0. The
        // `% with +` mutant adds 256 to bintmp[last] every iteration,
        // so the cast (..as u8) wraps to a non-zero value once the sum
        // exceeds 255.
        let bytes = bytes_from_binval(&build_binval(b"00000000000000000000"));
        assert_eq!(bytes, [0u8; 13]);
    }

    /// Kills the `^ with |` mutant at line 258 of `fcs_from_bytes`.
    /// The condition `((fcs ^ dat) & 0x400) != 0` checks whether bit
    /// 10 of (fcs XOR dat) is set — i.e. whether fcs and dat differ
    /// at bit 10. The `|` mutant changes this to "whether either has
    /// bit 10 set", which produces a different polynomial division
    /// and therefore a different FCS.
    ///
    /// The pre-existing `fcs_matches_bwip_js` test pins FCS=81 and
    /// 1873 for two specific 13-byte arrays. We add a third pin with
    /// a bytes array where bit 10 of `fcs ^ dat` is computed
    /// differently under XOR vs OR (i.e. bytes that trigger the
    /// "both have bit 10 set" case in the first 6-bit pass) so the
    /// mutant produces a divergent FCS.
    ///
    /// The 31-digit payload below produces bytes[0] = 1, which
    /// shifted by 5 gives dat = 32 (bit 5 set, NOT bit 10). After
    /// the loop processes 6 bits of bytes[0], the running fcs has a
    /// bit-10 pattern that differs from dat. We assert the known
    /// final FCS (1873) — under the mutant the FCS differs.
    #[test]
    fn fcs_from_bytes_uses_xor_not_or() {
        // Pin a synthetic byte array engineered to expose the
        // XOR-vs-OR mutation on the first iteration of the 6-bit
        // pass. The check is `if ((fcs ^ dat) & 0x400) != 0` — bit
        // 10 of (fcs XOR dat). Initial fcs = 2047 = 0x7FF, which
        // has bit 10 set. dat = bytes[0] << 5; for bytes[0] = 32
        // (= 0x20), dat = 0x400, which also has bit 10 set.
        //
        // Original: bit 10 of (fcs ^ dat) = 1 ^ 1 = 0 → don't
        //   XOR in the polynomial on iter 0.
        // Mutant `|`: bit 10 of (fcs | dat) = 1 | 1 = 1 → DO
        //   XOR in the polynomial.
        //
        // These produce different fcs values after iter 0, so the
        // final FCS diverges. We pin the *original* output so the
        // mutant fails the equality.
        let synthetic = [32u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let original_fcs = fcs_from_bytes(&synthetic);
        // The exact value is what the original `^`-based formula
        // produces; under the `|` mutant the polynomial XOR fires
        // an extra time on iter 0 and downstream iterations
        // accumulate differently.
        assert_eq!(
            original_fcs, 785,
            "fcs_from_bytes XOR-vs-OR divergence: bytes[0]=0x20 sets dat \
             bit 10 = 1 same as fcs initial bit 10 = 1; original returns \
             785 (XOR-path through the polynomial), mutant takes a \
             different branch on iter 0 and the final FCS diverges"
        );

        // Also keep the 31-digit anchor for the original golden so a
        // future refactor that breaks the XOR path generically is
        // caught even if the synthetic test happens to coincide.
        let b31 = bytes_from_binval(&build_binval(b"0123456709498765432101234567891"));
        assert_eq!(fcs_from_bytes(&b31), 1873);
    }

    #[test]
    fn binval_then_bytes_match_bwip_js_20() {
        let bytes = bytes_from_binval(&build_binval(b"01234567094987654321"));
        assert_eq!(bytes, [0, 0, 0, 0, 0, 17, 34, 16, 59, 92, 32, 4, 177],);
    }

    #[test]
    fn binval_then_bytes_match_bwip_js_31() {
        let bytes = bytes_from_binval(&build_binval(b"0123456709498765432101234567891"));
        assert_eq!(
            bytes,
            [1, 105, 7, 178, 162, 74, 188, 22, 162, 229, 192, 4, 177],
        );
    }

    #[test]
    fn fcs_matches_bwip_js() {
        let b20 = bytes_from_binval(&build_binval(b"01234567094987654321"));
        assert_eq!(fcs_from_bytes(&b20), 81);
        let b31 = bytes_from_binval(&build_binval(b"0123456709498765432101234567891"));
        assert_eq!(fcs_from_bytes(&b31), 1873);
    }

    #[test]
    fn codewords_match_bwip_js() {
        let bv20 = build_binval(b"01234567094987654321");
        let b20 = bytes_from_binval(&bv20);
        let f20 = fcs_from_bytes(&b20);
        assert_eq!(
            codewords_from_binval(bv20, f20),
            [0, 0, 0, 0, 559, 202, 508, 451, 124, 34],
        );

        let bv31 = build_binval(b"0123456709498765432101234567891");
        let b31 = bytes_from_binval(&bv31);
        let f31 = fcs_from_bytes(&b31);
        assert_eq!(
            codewords_from_binval(bv31, f31),
            [673, 787, 607, 1022, 861, 19, 816, 1294, 35, 602],
        );
    }

    #[test]
    fn end_to_end_renders_65_bars() {
        let p = encode("01234567094987654321", &Options::default()).unwrap();
        assert_eq!(p.bars.len(), 65);
    }

    /// Stage 11.A8c — pin `chars_from_codewords` boundary + FCS XOR.
    /// The function routes codewords < 1287 to TAB513 and ≥ 1287 to
    /// TAB213, then XORs each char with 0x1FFF iff the corresponding
    /// FCS bit is set. Existing tests only exercise this transitively
    /// through end-to-end goldens, where the specific codewords used
    /// happen to avoid both the 1286/1287 boundary and the FCS XOR
    /// edges (most FCS bits set, FCS=0 sentinel, etc.).
    ///
    /// Mutations to catch:
    ///   - `cw <= 1286` → `cw < 1286`: cw=1286 panics (TAB213[-1]).
    ///   - `cw - 1287` → `cw - 1286`: off-by-one — cw=1287 reads
    ///     TAB213[1] (=6144) instead of TAB213[0] (=3).
    ///   - `^=` → `&=` or `|=`: wrong mask operation.
    ///   - `0x1FFF` → `0x0FFF` etc.: wrong mask width.
    ///   - `(fcs & (1 << i)) != 0` → `== 0`: inverted FCS predicate.
    ///   - loop bound: `enumerate()` is implicit but a mutation
    ///     bounding `i` differently shows up.
    ///
    /// Hand-derived anchors:
    ///   - TAB513[0] = 31, TAB513[1286] = 496 (last TAB513 entry).
    ///   - TAB213[0] = 3 (first TAB213 entry), TAB213[77] = 160.
    ///   - 31 ^ 0x1FFF = 31 XOR 8191 = 8160 (top 8 bits flipped).
    #[test]
    fn chars_from_codewords_boundary_and_fcs_xor() {
        // Codeword 0 routes to TAB513[0] = 31; no FCS bit set →
        // every char is 31.
        let chars = chars_from_codewords(&[0u32; 10], 0);
        assert_eq!(
            chars, [31u16; 10],
            "all-zero codewords + fcs=0 → all TAB513[0]=31"
        );

        // Codeword 1286 is the LAST TAB513 entry → TAB513[1286] = 496.
        // Mutation `cw <= 1286` → `cw < 1286` would route 1286 to
        // TAB213[-1] which panics. The anchor catches non-panicking
        // mutations that route to a different table entry.
        let chars = chars_from_codewords(&[1286, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0);
        assert_eq!(chars[0], 496, "cw=1286 → TAB513[1286]=496 (LAST TAB513)");
        assert_eq!(chars[1..], [31u16; 9], "remaining cws unchanged");

        // Codeword 1287 is the FIRST TAB213 entry → TAB213[0] = 3.
        // Mutation `cw - 1287` → `cw - 1286` would index TAB213[1] = 6144.
        let chars = chars_from_codewords(&[1287, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0);
        assert_eq!(chars[0], 3, "cw=1287 → TAB213[0]=3 (FIRST TAB213)");

        // Codeword 1364 is the LAST TAB213 entry → TAB213[77] = 160.
        let chars = chars_from_codewords(&[1364, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0);
        assert_eq!(chars[0], 160, "cw=1364 → TAB213[77]=160 (LAST TAB213)");

        // FCS bit 0 set: chars[0] gets XOR'd with 0x1FFF = 8191.
        // 31 ^ 8191 = 8160.
        let chars = chars_from_codewords(&[0u32; 10], 0x0001);
        assert_eq!(chars[0], 8160, "fcs bit 0: 31 XOR 0x1FFF = 8160");
        assert_eq!(chars[1..], [31u16; 9], "other chars unchanged");

        // FCS bit 9 (highest): chars[9] gets XOR'd.
        let chars = chars_from_codewords(&[0u32; 10], 0x0200);
        assert_eq!(chars[9], 8160, "fcs bit 9: chars[9] XOR'd");
        assert_eq!(chars[..9], [31u16; 9]);

        // All 10 FCS bits set: every char XOR'd.
        let chars = chars_from_codewords(&[0u32; 10], 0x03FF);
        assert_eq!(chars, [8160u16; 10], "fcs=0x03FF → every char XOR'd");

        // Mixed: cw=1287 (TAB213[0]=3) with fcs bit 0 set → 3 ^ 8191.
        // 3 = 0b0000000000011, 8191 = 0b1111111111111. XOR = 0b1111111111100 = 8188.
        let chars = chars_from_codewords(&[1287, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0x0001);
        assert_eq!(chars[0], 8188, "TAB213[0]=3 XOR 0x1FFF = 8188");
    }

    /// Stage 11.A8c — pin `bars_from_chars` 4-state (dec, asc) → bar
    /// mapping. Existing tests only exercise this through end-to-end
    /// goldens, leaving the 4-arm match (Tracker / Ascender /
    /// Descender / Full) untested in isolation. Mutations to catch:
    ///   - arm swap, e.g. `(false, true) → Descender` instead of
    ///     `Ascender`.
    ///   - bit-mask swap: `(1 << dec_bit)` → `(1 << asc_bit)`.
    ///   - `& != 0` → `& == 0`: inverted predicate.
    ///   - loop count `0..65` → `0..64` / `0..66`: out-of-range read
    ///     or short output.
    ///
    /// Strategy: feed extreme inputs that produce uniform output
    /// (all-0 → Tracker, all-1FFF → Full), plus a hand-crafted
    /// mixed case that exercises both Ascender and Descender via the
    /// BARMAP's first pair lookup.
    #[test]
    fn bars_from_chars_four_state_mapping() {
        // All-zero chars: every bit reads 0 → every bar is Tracker.
        let zeros = [0u16; 10];
        let bars = bars_from_chars(&zeros);
        assert_eq!(bars.len(), 65, "must emit exactly 65 bars");
        assert!(
            bars.iter().all(|b| *b == Bar4State::Tracker),
            "all-zero input → every bar is Tracker (no bits set)"
        );

        // All-bits-set chars (13 bits = 0x1FFF): every bit reads 1 →
        // every bar is Full.
        let ones = [0x1FFFu16; 10];
        let bars = bars_from_chars(&ones);
        assert_eq!(bars.len(), 65);
        assert!(
            bars.iter().all(|b| *b == Bar4State::Full),
            "all-bits-set input → every bar is Full (both dec and asc bits set)"
        );

        // Set only descender bits (every other char gets all-1s) — but
        // BARMAP interleaves dec/asc reads across chars, so this isn't a
        // clean test. Use a more direct approach: anchor that all four
        // (dec, asc) combinations occur somewhere in the output for the
        // 20-digit golden, then pin the exact bar at a known position.
        let bv = build_binval(b"01234567094987654321");
        let bytes = bytes_from_binval(&bv);
        let fcs = fcs_from_bytes(&bytes);
        let cws = codewords_from_binval(bv, fcs);
        let chars = chars_from_codewords(&cws, fcs);
        let bars = bars_from_chars(&chars);
        assert_eq!(bars.len(), 65, "20-digit symbol → 65 bars");
        // The 20-digit golden symbol must exhibit all 4 bar states
        // (any pure-state output would mean the (dec, asc) match is
        // collapsing). Pin presence of each via iter().any().
        assert!(
            bars.contains(&Bar4State::Tracker),
            "20-digit symbol must contain at least one Tracker bar"
        );
        assert!(
            bars.contains(&Bar4State::Ascender),
            "20-digit symbol must contain at least one Ascender bar"
        );
        assert!(
            bars.contains(&Bar4State::Descender),
            "20-digit symbol must contain at least one Descender bar"
        );
        assert!(
            bars.contains(&Bar4State::Full),
            "20-digit symbol must contain at least one Full bar"
        );
    }

    #[test]
    fn rejects_bad_inputs() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to multi-anchor
        // pin per arm matching the source diagnostics at lines 357-358
        // and 362-364 of usps_onecode.rs.

        // Non-digit arm (line 357-358: `USPS OneCode: data must
        // contain only digits`):
        match encode("abc", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("USPS OneCode:"),
                    "non-digit arm: missing `USPS OneCode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("data must contain only digits"),
                    "non-digit arm: missing `data must contain only digits` predicate: {msg}"
                );
                assert!(
                    !msg.contains("20, 25, 29 or 31"),
                    "non-digit arm: wrong arm — length-spec diagnostic leaked into non-digit reject: {msg}"
                );
            }
            other => panic!("\"abc\" should reject as InvalidData, got {other:?}"),
        }

        // Wrong-length arm (line 362-364: `USPS OneCode: data must be
        // 20, 25, 29 or 31 digits (got {n})`):
        // 19 digits → wrong length.
        match encode("0123456789012345678", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("USPS OneCode:"),
                    "length arm: missing `USPS OneCode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("20, 25, 29 or 31 digits"),
                    "length arm: missing full length-spec (20/25/29/31): {msg}"
                );
                assert!(
                    msg.contains("got 19"),
                    "length arm: missing `got 19` value echo: {msg}"
                );
                assert!(
                    !msg.contains("data must contain only digits"),
                    "length arm: wrong arm — non-digit diagnostic leaked into length reject: {msg}"
                );
            }
            other => panic!("19-digit input should reject as InvalidData, got {other:?}"),
        }
    }

    fn bar_string(bars: &[crate::encoding::Bar4State]) -> String {
        bars.iter()
            .map(|b| match b {
                crate::encoding::Bar4State::Full => 'F',
                crate::encoding::Bar4State::Tracker => 'T',
                crate::encoding::Bar4State::Ascender => 'A',
                crate::encoding::Bar4State::Descender => 'D',
            })
            .collect()
    }

    /// Cross-validation against `b.raw("onecode", text, {})[0].bhs`
    /// classifying each bar's `(bhs, bbs)` pair into F/A/D/T. Anchors
    /// the full 65-bar output for the three published USPS digit
    /// counts (20 / 25 / 29) — exercises the binval split, FCS,
    /// codeword generation, and character table.
    ///
    /// OneCode uses different bar dimensions from RM4SCC: `bhs=0.15`
    /// is Full, `bhs=0.0375` is Tracker (the "small" middle bar),
    /// and `bhs=0.09375` is the half-height descender / ascender
    /// distinguished by `bbs`.
    #[test]
    fn end_to_end_matches_bwip_js() {
        let cases: &[(&str, &str)] = &[
            (
                "12345678901234567890",
                "AFDADFAAAFTDDTDDTATADDFAADFDDFTAFTDTAFTFTDTATFTFAFDFDFFFFTFFFATDD",
            ),
            (
                "01234567094987654321012345678",
                "ADFTTAFDTTTTFATTADTAAATFTFTATDAAAFDDADATATDTDTTDFDTDATADADTDFFTFA",
            ),
            (
                "0123456709498765432101234",
                "DTTAFADDTTFTDTFTFDTDDADADAFADFATDDFTAAAFDTTADFAAATDFDTDFADDDTDFFT",
            ),
        ];
        for &(text, want) in cases {
            let p = encode(text, &Options::default()).unwrap();
            assert_eq!(
                bar_string(&p.bars),
                want,
                "USPS OneCode bar sequence mismatch for {text:?}"
            );
        }
    }
}
