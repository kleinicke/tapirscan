// Independent EAN-13 writer: 4006381333931, first-digit parity LGLLGG.
export function fixture() {
  const L = [
    "0001101",
    "0011001",
    "0010011",
    "0111101",
    "0100011",
    "0110001",
    "0101111",
    "0111011",
    "0110111",
    "0001011",
  ];
  const G = [
    "0100111",
    "0110011",
    "0011011",
    "0100001",
    "0011101",
    "0111001",
    "0000101",
    "0010001",
    "0001001",
    "0010111",
  ];
  const text = "4006381333931";
  let bits = "101";
  for (let i = 0; i < 6; i++) bits += ("LGLLGG"[i] === "L" ? L : G)[Number(text[i + 1])];
  bits += "01010";
  for (let i = 7; i < 13; i++)
    bits += L[Number(text[i])].replace(/[01]/g, (x) => (x === "0" ? "1" : "0"));
  bits += "101";
  const width = 480,
    height = 180,
    data = new Uint8Array(width * height).fill(255);
  for (let y = 30; y < 150; y++)
    for (let i = 0; i < bits.length; i++)
      if (bits[i] === "1") data.fill(0, y * width + 50 + i * 4, y * width + 54 + i * 4);
  return { text, image: { data, width, height, channels: 1, stride: width } };
}

export function ean8(copies = 1, gap = 1, text = "96385074") {
  const digits = [
    "0001101",
    "0011001",
    "0010011",
    "0111101",
    "0100011",
    "0110001",
    "0101111",
    "0111011",
    "0110111",
    "0001011",
  ];
  const bits =
    "101" +
    [...text.slice(0, 4)].map((d) => digits[Number(d)]).join("") +
    "01010" +
    [...text.slice(4)]
      .map((d) => digits[Number(d)].replace(/[01]/g, (bit) => (bit === "0" ? "1" : "0")))
      .join("") +
    "101";
  const width = 340,
    height = 40 + copies * 80 + (copies - 1) * gap;
  const data = new Uint8Array(width * height).fill(255);
  for (let copy = 0; copy < copies; copy++)
    for (let y = 20 + copy * (80 + gap); y < 100 + copy * (80 + gap); y++)
      for (let x = 0; x < bits.length; x++)
        if (bits[x] === "1") data.fill(0, y * width + 36 + x * 4, y * width + 40 + x * 4);
  return { data, width, height, channels: 1 };
}
