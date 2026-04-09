HEX_TABLE = {
    0: "春风入夜",
    1: "明月照山",
    2: "云起青峰",
    3: "山雨初来",
    4: "花落无声",
    5: "梅影横窗",
    6: "竹深听雨",
    7: "书灯夜静",
    8: "归舟江晚",
    9: "风过松林",
    10: "雨润春山",
    11: "花开小径",
    12: "鸟鸣空谷",
    13: "月上寒江",
    14: "云散天青",
    15: "天远星沉",
}

DECODE_TABLE = {v: k for k, v in HEX_TABLE.items()}


def encode(text: str) -> str:
    data = text.encode("utf-8")
    lines = []

    for b in data:
        hi = b >> 4
        lo = b & 0xF

        lines.append(HEX_TABLE[hi])
        lines.append(HEX_TABLE[lo])

    return "\n".join(lines)


print(encode("我也很喜欢和你一起玩"))
