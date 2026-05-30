const bwipjs = require("bwip-js");
const head = "JGB 012100123412345678AB19XY1A";
const text = head + " ".repeat(90 - head.length);
console.log("len:", text.length);
(async () => {
  try {
    const png = await bwipjs.toBuffer({ bcid: "mailmark", text, type: "29", scale: 1 });
    console.log("OK, png size:", png.length);
  } catch (e) {
    console.error("err:", e.message);
  }
})();
