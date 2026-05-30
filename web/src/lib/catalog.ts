export type CatalogEntry = {
  id: string;
  name: string;
  category: string;
  renderer: 'bwipp' | 'custom';
  bwippType: string | null;
  options: Record<string, unknown>;
  example: string;
  customHandler: string | null;
  notes: string;
};

export const categories = [
  "1D - Standard",
  "1D - 2 of 5 family",
  "1D - Retail / EAN / UPC",
  "1D - Specialized",
  "1D - Pharmaceutical",
  "1D - ISBN / Media",
  "1D - GS1 DataBar",
  "Postal",
  "2D - Matrix",
  "2D - Stacked / Multi-row",
  "2D - Specialty",
  "GS1 DataBar Stacked",
  "HIBC (Healthcare)",
  "Composite (Linear + 2D)"
] as const;

export const catalog = [
  {
    "id": "code39",
    "name": "Code 39",
    "category": "1D - Standard",
    "renderer": "bwipp",
    "bwippType": "code39",
    "options": {},
    "example": "HELLO-123",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "code39ext",
    "name": "Code 39 Full ASCII",
    "category": "1D - Standard",
    "renderer": "bwipp",
    "bwippType": "code39ext",
    "options": {},
    "example": "Hello123",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "code93",
    "name": "Code 93",
    "category": "1D - Standard",
    "renderer": "bwipp",
    "bwippType": "code93",
    "options": {},
    "example": "CODE93",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "code93ext",
    "name": "Code 93 Full ASCII",
    "category": "1D - Standard",
    "renderer": "bwipp",
    "bwippType": "code93ext",
    "options": {},
    "example": "Code 93 ext",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "code11",
    "name": "Code 11",
    "category": "1D - Standard",
    "renderer": "bwipp",
    "bwippType": "code11",
    "options": {},
    "example": "0123456789",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "code128",
    "name": "Code 128",
    "category": "1D - Standard",
    "renderer": "bwipp",
    "bwippType": "code128",
    "options": {},
    "example": "Code 128",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "code128a",
    "name": "Code 128 Subset A",
    "category": "1D - Standard",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "subset": "A"
    },
    "example": "HELLO 128A",
    "customHandler": "render_code128_subset",
    "notes": ""
  },
  {
    "id": "code128b",
    "name": "Code 128 Subset B",
    "category": "1D - Standard",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "subset": "B"
    },
    "example": "Hello 128B",
    "customHandler": "render_code128_subset",
    "notes": ""
  },
  {
    "id": "code128c",
    "name": "Code 128 Subset C",
    "category": "1D - Standard",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "subset": "C"
    },
    "example": "123456",
    "customHandler": "render_code128_subset",
    "notes": ""
  },
  {
    "id": "code32",
    "name": "Code 32 (Italian Pharmacode)",
    "category": "1D - Standard",
    "renderer": "bwipp",
    "bwippType": "code32",
    "options": {},
    "example": "01234567",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "code2of5",
    "name": "Code 2 of 5 (Standard)",
    "category": "1D - 2 of 5 family",
    "renderer": "bwipp",
    "bwippType": "code2of5",
    "options": {},
    "example": "123456",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "datalogic2of5",
    "name": "Code 2 of 5 Data Logic",
    "category": "1D - 2 of 5 family",
    "renderer": "bwipp",
    "bwippType": "datalogic2of5",
    "options": {},
    "example": "123456",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "iata2of5",
    "name": "Code 2 of 5 IATA",
    "category": "1D - 2 of 5 family",
    "renderer": "bwipp",
    "bwippType": "iata2of5",
    "options": {},
    "example": "12345678",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "industrial2of5",
    "name": "Code 2 of 5 Industry",
    "category": "1D - 2 of 5 family",
    "renderer": "bwipp",
    "bwippType": "industrial2of5",
    "options": {},
    "example": "123456",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "interleaved2of5",
    "name": "Code 2 of 5 Interleaved",
    "category": "1D - 2 of 5 family",
    "renderer": "bwipp",
    "bwippType": "interleaved2of5",
    "options": {},
    "example": "12345678",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "matrix2of5",
    "name": "Code 2 of 5 Matrix",
    "category": "1D - 2 of 5 family",
    "renderer": "bwipp",
    "bwippType": "matrix2of5",
    "options": {},
    "example": "123456",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "coop2of5",
    "name": "Code 2 of 5 COOP",
    "category": "1D - 2 of 5 family",
    "renderer": "bwipp",
    "bwippType": "coop2of5",
    "options": {},
    "example": "123456",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "ean13",
    "name": "EAN-13",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "bwipp",
    "bwippType": "ean13",
    "options": {},
    "example": "012345678905",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "ean13p2",
    "name": "EAN-13 + 2-digit add-on",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "ean13",
      "addon_len": 2
    },
    "example": "012345678905 12",
    "customHandler": "render_ean_addon",
    "notes": ""
  },
  {
    "id": "ean13p5",
    "name": "EAN-13 + 5-digit add-on",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "ean13",
      "addon_len": 5
    },
    "example": "012345678905 12345",
    "customHandler": "render_ean_addon",
    "notes": ""
  },
  {
    "id": "ean8",
    "name": "EAN-8",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "bwipp",
    "bwippType": "ean8",
    "options": {},
    "example": "1234567",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "ean8p2",
    "name": "EAN-8 + 2-digit add-on",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "ean8",
      "addon_len": 2,
      "permitaddon": true
    },
    "example": "1234567 12",
    "customHandler": "render_ean_addon",
    "notes": ""
  },
  {
    "id": "ean8p5",
    "name": "EAN-8 + 5-digit add-on",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "ean8",
      "addon_len": 5,
      "permitaddon": true
    },
    "example": "1234567 12345",
    "customHandler": "render_ean_addon",
    "notes": ""
  },
  {
    "id": "upca",
    "name": "UPC-A",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "bwipp",
    "bwippType": "upca",
    "options": {},
    "example": "01234567890",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "upcap2",
    "name": "UPC-A + 2-digit add-on",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "upca",
      "addon_len": 2
    },
    "example": "01234567890 12",
    "customHandler": "render_ean_addon",
    "notes": ""
  },
  {
    "id": "upcap5",
    "name": "UPC-A + 5-digit add-on",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "upca",
      "addon_len": 5
    },
    "example": "01234567890 12345",
    "customHandler": "render_ean_addon",
    "notes": ""
  },
  {
    "id": "upce",
    "name": "UPC-E",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "bwipp",
    "bwippType": "upce",
    "options": {},
    "example": "01234565",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "upcep2",
    "name": "UPC-E + 2-digit add-on",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "upce",
      "addon_len": 2
    },
    "example": "01234565 12",
    "customHandler": "render_ean_addon",
    "notes": ""
  },
  {
    "id": "upcep5",
    "name": "UPC-E + 5-digit add-on",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "upce",
      "addon_len": 5
    },
    "example": "01234565 12345",
    "customHandler": "render_ean_addon",
    "notes": ""
  },
  {
    "id": "gs1-128",
    "name": "EAN-128 / GS1-128",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "bwipp",
    "bwippType": "gs1-128",
    "options": {
      "parse": true
    },
    "example": "(01)04012345123456(17)260101",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "ucc128",
    "name": "UCC-128",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "bwipp",
    "bwippType": "gs1-128",
    "options": {
      "parse": true
    },
    "example": "(01)04012345123456",
    "customHandler": null,
    "notes": "UCC-128 is the legacy name for GS1-128; rendered identically."
  },
  {
    "id": "upc_coupon",
    "name": "UPC Coupon Code",
    "category": "1D - Retail / EAN / UPC",
    "renderer": "bwipp",
    "bwippType": "gs1northamericancoupon",
    "options": {
      "parse": true,
      "segments": "8"
    },
    "example": "(8110)106141416543213500110000310123196000",
    "customHandler": null,
    "notes": "GS1 North American Coupon Code (AI 8110)."
  },
  {
    "id": "codabar",
    "name": "Codabar",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "rationalizedCodabar",
    "options": {},
    "example": "A12345B",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "itf14",
    "name": "ITF-14",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "itf14",
    "options": {},
    "example": "1234567890123",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "msi",
    "name": "MSI",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "msi",
    "options": {},
    "example": "123456",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "plessey",
    "name": "Plessey",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "plessey",
    "options": {},
    "example": "01234ABCD",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "plessey_bidir",
    "name": "Plessey Bidirectional",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "plessey",
    "options": {
      "unidirectional": false
    },
    "example": "01234ABCD",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "telepen",
    "name": "Telepen",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "telepen",
    "options": {},
    "example": "Hello",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "telepen_alpha",
    "name": "Telepen Alpha",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "telepennumeric",
    "options": {},
    "example": "123456",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "vin",
    "name": "VIN / FIN",
    "category": "1D - Specialized",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "1HGCM82633A123456",
    "customHandler": "render_vin",
    "notes": "17-character Vehicle Identification Number. Validated then rendered as Code 39."
  },
  {
    "id": "logmars",
    "name": "LOGMARS",
    "category": "1D - Specialized",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "LOGMARS123",
    "customHandler": "render_logmars",
    "notes": "US DoD MIL-STD-1189B; subset of Code 39."
  },
  {
    "id": "sscc18",
    "name": "SSCC-18",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "sscc18",
    "options": {
      "parse": true
    },
    "example": "(00)106141411234567897",
    "customHandler": null,
    "notes": "GS1 SSCC with AI (00). 18 digits including a valid mod-10 check digit."
  },
  {
    "id": "nve18",
    "name": "NVE-18",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "sscc18",
    "options": {
      "parse": true
    },
    "example": "(00)106141411234567897",
    "customHandler": null,
    "notes": "NVE-18 is the German name for SSCC-18; rendered identically."
  },
  {
    "id": "flattermarken",
    "name": "Flattermarken",
    "category": "1D - Specialized",
    "renderer": "bwipp",
    "bwippType": "flattermarken",
    "options": {},
    "example": "1234567",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "pharmacode",
    "name": "Pharmacode One-Track",
    "category": "1D - Pharmaceutical",
    "renderer": "bwipp",
    "bwippType": "pharmacode",
    "options": {},
    "example": "117",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "pharmacode2",
    "name": "Pharmacode Two-Track",
    "category": "1D - Pharmaceutical",
    "renderer": "bwipp",
    "bwippType": "pharmacode2",
    "options": {},
    "example": "117",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "pzn7",
    "name": "PZN7",
    "category": "1D - Pharmaceutical",
    "renderer": "bwipp",
    "bwippType": "pzn",
    "options": {
      "pzn8": false
    },
    "example": "123456",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "pzn8",
    "name": "PZN8",
    "category": "1D - Pharmaceutical",
    "renderer": "bwipp",
    "bwippType": "pzn",
    "options": {
      "pzn8": true
    },
    "example": "1234567",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "isbn13",
    "name": "ISBN-13",
    "category": "1D - ISBN / Media",
    "renderer": "bwipp",
    "bwippType": "isbn",
    "options": {},
    "example": "978-1-56619-909-4",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "isbn13p5",
    "name": "ISBN-13 + 5-digit add-on",
    "category": "1D - ISBN / Media",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "addon_len": 5
    },
    "example": "978-1-56619-909-4 50995",
    "customHandler": "render_isbn_addon",
    "notes": ""
  },
  {
    "id": "ismn",
    "name": "ISMN",
    "category": "1D - ISBN / Media",
    "renderer": "bwipp",
    "bwippType": "ismn",
    "options": {},
    "example": "979-0-1234-5678-5",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "issn",
    "name": "ISSN",
    "category": "1D - ISBN / Media",
    "renderer": "bwipp",
    "bwippType": "issn",
    "options": {},
    "example": "0317-8471",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "issnp2",
    "name": "ISSN + 2-digit add-on",
    "category": "1D - ISBN / Media",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "addon_len": 2
    },
    "example": "0317-8471 13",
    "customHandler": "render_issn_addon",
    "notes": ""
  },
  {
    "id": "databar_omni",
    "name": "GS1 DataBar Omnidirectional",
    "category": "1D - GS1 DataBar",
    "renderer": "bwipp",
    "bwippType": "databaromni",
    "options": {},
    "example": "(01)24012345678905",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "databar_expanded",
    "name": "GS1 DataBar Expanded",
    "category": "1D - GS1 DataBar",
    "renderer": "bwipp",
    "bwippType": "databarexpanded",
    "options": {
      "parse": true
    },
    "example": "(01)90012345678908(3103)001750",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "databar_truncated",
    "name": "GS1 DataBar Truncated",
    "category": "1D - GS1 DataBar",
    "renderer": "bwipp",
    "bwippType": "databartruncated",
    "options": {},
    "example": "(01)24012345678905",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "databar_limited",
    "name": "GS1 DataBar Limited",
    "category": "1D - GS1 DataBar",
    "renderer": "bwipp",
    "bwippType": "databarlimited",
    "options": {},
    "example": "(01)15012345678907",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "auspost_customer",
    "name": "Australia Post 4-State (Customer)",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "fcc": "11"
    },
    "example": "12345678",
    "customHandler": "render_auspost",
    "notes": "FCC 11 - Standard Customer. Provide the 8-digit DPID; the FCC is prepended for you."
  },
  {
    "id": "auspost_reply",
    "name": "Australia Post 4-State (Reply Paid)",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "fcc": "45"
    },
    "example": "12345678",
    "customHandler": "render_auspost",
    "notes": "FCC 45 - Reply Paid. 8-digit DPID."
  },
  {
    "id": "auspost_routing",
    "name": "Australia Post 4-State (Routing)",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "fcc": "59"
    },
    "example": "12345678",
    "customHandler": "render_auspost",
    "notes": "FCC 59 - Routing/Customer 2. 8-digit DPID + optional customer info."
  },
  {
    "id": "auspost_redirection",
    "name": "Australia Post 4-State (Redirection)",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "fcc": "62"
    },
    "example": "12345678",
    "customHandler": "render_auspost",
    "notes": "FCC 62 - Redirection/Customer 3. 8-digit DPID + optional customer info."
  },
  {
    "id": "cepnet",
    "name": "Brazilian CEPNet",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "12345678",
    "customHandler": "render_cepnet",
    "notes": "Brazilian postal code rendered as PostNet variant."
  },
  {
    "id": "daft",
    "name": "DAFT Code",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "daft",
    "options": {},
    "example": "DAFTDAFTDAFT",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "dpd",
    "name": "DPD",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "%000393060781000300000110001020796",
    "customHandler": "render_dpd",
    "notes": "DPD parcel barcode (Code 128 with DPD-specific format)."
  },
  {
    "id": "identcode",
    "name": "DP Identcode",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "identcode",
    "options": {},
    "example": "34567890123",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "leitcode",
    "name": "DP Leitcode",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "leitcode",
    "options": {},
    "example": "12345678901236",
    "customHandler": null,
    "notes": "13 data digits + Deutsche Post check digit (computed mod-10 with 4-9 weighting)."
  },
  {
    "id": "italian_postal_25",
    "name": "Italian Postal 2 of 5",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "12345678",
    "customHandler": "render_italian_postal_25",
    "notes": "Italian postal Interleaved 2 of 5 variant."
  },
  {
    "id": "italian_postal_39",
    "name": "Italian Postal 3 of 9",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "12345678",
    "customHandler": "render_italian_postal_39",
    "notes": "Italian postal Code 39 variant."
  },
  {
    "id": "japanpost",
    "name": "Japan Post",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "japanpost",
    "options": {},
    "example": "123-4567-890",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "kix",
    "name": "KIX (Klant Index)",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "kix",
    "options": {},
    "example": "2500GG30",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "korean_postal",
    "name": "Korean Postal Authority",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "123456",
    "customHandler": "render_korean_postal",
    "notes": "6-digit Korean postal code. Rendered as Code 128 with check digit."
  },
  {
    "id": "royalmail",
    "name": "Royal Mail 4-State (RM4SCC)",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "royalmail",
    "options": {},
    "example": "LE28HS9Z",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "mailmark",
    "name": "Royal Mail Mailmark",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "mailmark",
    "options": {
      "type": "29"
    },
    "example": "JGB 012100123412345678AB19XY1A 0             www.xyz.com",
    "customHandler": null,
    "notes": "Mailmark 4-state. The `type` option (7, 9, or 29) determines bar density. Data must be at least 45 chars."
  },
  {
    "id": "mailmark2d",
    "name": "Royal Mail Mailmark 2D",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "JGB 012100123412345678AB19XY1A               ",
    "customHandler": "render_mailmark_2d",
    "notes": "Mailmark 2D - 45/70/90-char payload encoded as Data Matrix (square for 45/70, 16x48 rectangular for 90)."
  },
  {
    "id": "swedish_postal",
    "name": "Swedish Postal Shipment Item ID",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "37241718000000000",
    "customHandler": "render_swedish_postal",
    "notes": "Swedish Posten Shipment ID rendered as GS1-128 with AI (00). Accepts 17 digits (check auto-computed) or 18 digits (check included)."
  },
  {
    "id": "upu_s10",
    "name": "UPU S10",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "RA123456785US",
    "customHandler": "render_upu_s10",
    "notes": "UPU S10 international tracking number (13 chars). Rendered as Code 128."
  },
  {
    "id": "usps_onecode",
    "name": "USPS OneCode / Intelligent Mail",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "onecode",
    "options": {},
    "example": "0123456709498765432101234567891",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "usps_imb",
    "name": "USPS Intelligent Mail (IMb)",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "onecode",
    "options": {},
    "example": "0123456709498765432101234567891",
    "customHandler": null,
    "notes": "Alias for USPS OneCode."
  },
  {
    "id": "usps_impb",
    "name": "USPS Intelligent Mail Package",
    "category": "Postal",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "(420)94401(92)0040145914745473030413",
    "customHandler": "render_usps_impb",
    "notes": "USPS Intelligent Mail Package Barcode (IMpb), GS1-128 based."
  },
  {
    "id": "usps_postnet5",
    "name": "USPS PostNet (5 digit / 6 bars)",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "postnet",
    "options": {},
    "example": "12345",
    "customHandler": null,
    "notes": "5 data digits + auto-computed mod-10 check = 6 bars."
  },
  {
    "id": "usps_postnet9",
    "name": "USPS PostNet (9 digit / 10 bars)",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "postnet",
    "options": {},
    "example": "123456789",
    "customHandler": null,
    "notes": "9 data digits + auto-computed check = 10 bars."
  },
  {
    "id": "usps_postnet11",
    "name": "USPS PostNet (11 digit / 12 bars)",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "postnet",
    "options": {},
    "example": "12345678901",
    "customHandler": null,
    "notes": "11 data digits + auto-computed check = 12 bars."
  },
  {
    "id": "planet12",
    "name": "PLANET (12 digit)",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "planet",
    "options": {},
    "example": "12345678901",
    "customHandler": null,
    "notes": "11 data digits + BWIPP-computed check = 12 bars."
  },
  {
    "id": "planet14",
    "name": "PLANET (14 digit)",
    "category": "Postal",
    "renderer": "bwipp",
    "bwippType": "planet",
    "options": {},
    "example": "1234567890123",
    "customHandler": null,
    "notes": "13 data digits + BWIPP-computed check = 14 bars."
  },
  {
    "id": "azteccode",
    "name": "Aztec Code",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "azteccode",
    "options": {},
    "example": "Hello Aztec",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "datamatrix",
    "name": "Data Matrix (ECC200)",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "datamatrix",
    "options": {},
    "example": "Hello Data Matrix",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "gs1datamatrix",
    "name": "GS1 DataMatrix",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "gs1datamatrix",
    "options": {
      "parse": true
    },
    "example": "(01)04012345123456(17)260101",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "dotcode",
    "name": "DotCode",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "dotcode",
    "options": {},
    "example": "Hello DotCode",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "hanxin",
    "name": "Han Xin Code",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "hanxin",
    "options": {},
    "example": "HelloHanXin",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "code16k",
    "name": "Code 16K",
    "category": "2D - Stacked",
    "renderer": "bwipp",
    "bwippType": "code16k",
    "options": {},
    "example": "12",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "code49",
    "name": "Code 49",
    "category": "2D - Stacked",
    "renderer": "bwipp",
    "bwippType": "code49",
    "options": {},
    "example": "12345",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "codeone",
    "name": "Code One",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "codeone",
    "options": {},
    "example": "Hello",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "microqrcode",
    "name": "Micro QR Code",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "microqrcode",
    "options": {},
    "example": "Hello",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "qrcode",
    "name": "QR Code (JIS)",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "qrcode",
    "options": {},
    "example": "https://example.com",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "qrcode_iso",
    "name": "QR Code (ISO/IEC 18004:2015)",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "qrcode",
    "options": {
      "eclevel": "M"
    },
    "example": "https://example.com",
    "customHandler": null,
    "notes": "Modern QR Code with explicit error-correction level M."
  },
  {
    "id": "swissqrcode",
    "name": "Swiss QR Code",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "swissqrcode",
    "options": {},
    "example": "SPC\n0200\n1\nCH4431999123000889012\nS\nMax Muster\nMustergasse\n22\n8000\nZuerich\nCH\n\n\n\n\n\n\n\n100.00\nCHF\nS\nSimone Muster\nMusterstrasse\n1\n8000\nZuerich\nCH\nNON\n\nThank you\nEPD",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "codablockf",
    "name": "Codablock-F",
    "category": "2D - Stacked / Multi-row",
    "renderer": "bwipp",
    "bwippType": "codablockf",
    "options": {},
    "example": "Codablock F",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "pdf417",
    "name": "PDF417",
    "category": "2D - Stacked / Multi-row",
    "renderer": "bwipp",
    "bwippType": "pdf417",
    "options": {},
    "example": "Hello PDF417",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "pdf417_truncated",
    "name": "PDF417 Truncated",
    "category": "2D - Stacked / Multi-row",
    "renderer": "bwipp",
    "bwippType": "pdf417",
    "options": {
      "compact": true
    },
    "example": "Hello PDF417",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "micropdf417",
    "name": "Micro PDF417",
    "category": "2D - Stacked / Multi-row",
    "renderer": "bwipp",
    "bwippType": "micropdf417",
    "options": {},
    "example": "MicroPDF",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "dp_postmatrix",
    "name": "DP Postmatrix",
    "category": "2D - Specialty",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "0123456789012345",
    "customHandler": "render_dp_postmatrix",
    "notes": "Deutsche Post Matrix - rendered as Data Matrix with DP format."
  },
  {
    "id": "maxicode",
    "name": "MaxiCode",
    "category": "2D - Specialty",
    "renderer": "bwipp",
    "bwippType": "maxicode",
    "options": {},
    "example": "[)>\u001e01\u001d96152382802\u001d840\u001d001\u001d1Z00004951\u001d",
    "customHandler": null,
    "notes": "UPS MaxiCode mode 2; supply structured carrier data."
  },
  {
    "id": "ntin",
    "name": "NTIN",
    "category": "2D - Specialty",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "00012345678905",
    "customHandler": "render_ntin",
    "notes": "National Trade Item Number rendered as Data Matrix with AI (8003). Pass a 14-digit GTIN-14."
  },
  {
    "id": "ppn",
    "name": "PPN",
    "category": "2D - Specialty",
    "renderer": "custom",
    "bwippType": null,
    "options": {},
    "example": "110375286414",
    "customHandler": "render_ppn",
    "notes": "Pharmacy Product Number rendered as Data Matrix with FNC1 + AI 9N."
  },
  {
    "id": "databar_stacked",
    "name": "GS1 DataBar Stacked",
    "category": "GS1 DataBar Stacked",
    "renderer": "bwipp",
    "bwippType": "databarstacked",
    "options": {},
    "example": "(01)24012345678905",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "databar_stacked_omni",
    "name": "GS1 DataBar Stacked Omnidirectional",
    "category": "GS1 DataBar Stacked",
    "renderer": "bwipp",
    "bwippType": "databarstackedomni",
    "options": {},
    "example": "(01)24012345678905",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "databar_expanded_stacked",
    "name": "GS1 DataBar Expanded Stacked",
    "category": "GS1 DataBar Stacked",
    "renderer": "bwipp",
    "bwippType": "databarexpandedstacked",
    "options": {
      "parse": true
    },
    "example": "(01)90012345678908(3103)001750",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "hibc_lic_code128",
    "name": "HIBC LIC - Code 128",
    "category": "HIBC (Healthcare)",
    "renderer": "bwipp",
    "bwippType": "hibccode128",
    "options": {},
    "example": "A99912345/$$52001510X3",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "hibc_lic_code39",
    "name": "HIBC LIC - Code 39",
    "category": "HIBC (Healthcare)",
    "renderer": "bwipp",
    "bwippType": "hibccode39",
    "options": {},
    "example": "A99912345/$$52001510X3",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "hibc_lic_codablockf",
    "name": "HIBC LIC - Codablock F",
    "category": "HIBC (Healthcare)",
    "renderer": "bwipp",
    "bwippType": "hibccodablockf",
    "options": {},
    "example": "A99912345/$$52001510X3",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "hibc_lic_datamatrix",
    "name": "HIBC LIC - Data Matrix",
    "category": "HIBC (Healthcare)",
    "renderer": "bwipp",
    "bwippType": "hibcdatamatrix",
    "options": {},
    "example": "A99912345/$$52001510X3",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "hibc_lic_micropdf417",
    "name": "HIBC LIC - MicroPDF417",
    "category": "HIBC (Healthcare)",
    "renderer": "bwipp",
    "bwippType": "hibcmicropdf417",
    "options": {},
    "example": "A99912345/$$52001510X3",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "hibc_lic_pdf417",
    "name": "HIBC LIC - PDF417",
    "category": "HIBC (Healthcare)",
    "renderer": "bwipp",
    "bwippType": "hibcpdf417",
    "options": {},
    "example": "A99912345/$$52001510X3",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "hibc_lic_qrcode",
    "name": "HIBC LIC - QR Code",
    "category": "HIBC (Healthcare)",
    "renderer": "bwipp",
    "bwippType": "hibcqrcode",
    "options": {},
    "example": "A99912345/$$52001510X3",
    "customHandler": null,
    "notes": ""
  },
  {
    "id": "hibc_pas_code128",
    "name": "HIBC PAS - Code 128",
    "category": "HIBC (Healthcare)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "bwipp_type": "hibccode128"
    },
    "example": "A/99912345/$$52001510X3",
    "customHandler": "render_hibc_pas",
    "notes": ""
  },
  {
    "id": "hibc_pas_code39",
    "name": "HIBC PAS - Code 39",
    "category": "HIBC (Healthcare)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "bwipp_type": "hibccode39"
    },
    "example": "A/99912345/$$52001510X3",
    "customHandler": "render_hibc_pas",
    "notes": ""
  },
  {
    "id": "hibc_pas_codablockf",
    "name": "HIBC PAS - Codablock F",
    "category": "HIBC (Healthcare)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "bwipp_type": "hibccodablockf"
    },
    "example": "A/99912345/$$52001510X3",
    "customHandler": "render_hibc_pas",
    "notes": ""
  },
  {
    "id": "hibc_pas_datamatrix",
    "name": "HIBC PAS - Data Matrix",
    "category": "HIBC (Healthcare)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "bwipp_type": "hibcdatamatrix"
    },
    "example": "A/99912345/$$52001510X3",
    "customHandler": "render_hibc_pas",
    "notes": ""
  },
  {
    "id": "hibc_pas_micropdf417",
    "name": "HIBC PAS - MicroPDF417",
    "category": "HIBC (Healthcare)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "bwipp_type": "hibcmicropdf417"
    },
    "example": "A/99912345/$$52001510X3",
    "customHandler": "render_hibc_pas",
    "notes": ""
  },
  {
    "id": "hibc_pas_pdf417",
    "name": "HIBC PAS - PDF417",
    "category": "HIBC (Healthcare)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "bwipp_type": "hibcpdf417"
    },
    "example": "A/99912345/$$52001510X3",
    "customHandler": "render_hibc_pas",
    "notes": ""
  },
  {
    "id": "hibc_pas_qrcode",
    "name": "HIBC PAS - QR Code",
    "category": "HIBC (Healthcare)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "bwipp_type": "hibcqrcode"
    },
    "example": "A/99912345/$$52001510X3",
    "customHandler": "render_hibc_pas",
    "notes": ""
  },
  {
    "id": "composite_ean13_cca",
    "name": "EAN-13 Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "ean13composite",
      "cc": "A"
    },
    "example": "012345678905|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_ean13_ccb",
    "name": "EAN-13 Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "ean13composite",
      "cc": "B"
    },
    "example": "012345678905|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_ean8_cca",
    "name": "EAN-8 Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "ean8composite",
      "cc": "A"
    },
    "example": "1234567|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_ean8_ccb",
    "name": "EAN-8 Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "ean8composite",
      "cc": "B"
    },
    "example": "1234567|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_upca_cca",
    "name": "UPC-A Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "upcacomposite",
      "cc": "A"
    },
    "example": "01234567890|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_upca_ccb",
    "name": "UPC-A Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "upcacomposite",
      "cc": "B"
    },
    "example": "01234567890|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_upce_cca",
    "name": "UPC-E Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "upcecomposite",
      "cc": "A"
    },
    "example": "01234565|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_upce_ccb",
    "name": "UPC-E Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "upcecomposite",
      "cc": "B"
    },
    "example": "01234565|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_gs1_128_cca",
    "name": "GS1-128 Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "gs1-128composite",
      "cc": "A"
    },
    "example": "(01)04012345123456|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_gs1_128_ccb",
    "name": "GS1-128 Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "gs1-128composite",
      "cc": "B"
    },
    "example": "(01)04012345123456|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_gs1_128_ccc",
    "name": "GS1-128 Composite (CC-C)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "gs1-128composite",
      "cc": "C"
    },
    "example": "(01)04012345123456|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_omni_cca",
    "name": "GS1 DataBar Omni Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databaromnicomposite",
      "cc": "A"
    },
    "example": "(01)24012345678905|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_omni_ccb",
    "name": "GS1 DataBar Omni Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databaromnicomposite",
      "cc": "B"
    },
    "example": "(01)24012345678905|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_truncated_cca",
    "name": "GS1 DataBar Truncated Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databartruncatedcomposite",
      "cc": "A"
    },
    "example": "(01)24012345678905|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_truncated_ccb",
    "name": "GS1 DataBar Truncated Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databartruncatedcomposite",
      "cc": "B"
    },
    "example": "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_stacked_cca",
    "name": "GS1 DataBar Stacked Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarstackedcomposite",
      "cc": "A"
    },
    "example": "(01)24012345678905|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_stacked_ccb",
    "name": "GS1 DataBar Stacked Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarstackedcomposite",
      "cc": "B"
    },
    "example": "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_stacked_omni_cca",
    "name": "GS1 DataBar Stacked Omni Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarstackedomnicomposite",
      "cc": "A"
    },
    "example": "(01)24012345678905|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_stacked_omni_ccb",
    "name": "GS1 DataBar Stacked Omni Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarstackedomnicomposite",
      "cc": "B"
    },
    "example": "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_expanded_stacked_cca",
    "name": "GS1 DataBar Expanded Stacked Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarexpandedstackedcomposite",
      "cc": "A"
    },
    "example": "(01)90012345678908(3103)001750|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_expanded_stacked_ccb",
    "name": "GS1 DataBar Expanded Stacked Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarexpandedstackedcomposite",
      "cc": "B"
    },
    "example": "(01)90012345678908(3103)001750|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_expanded_cca",
    "name": "GS1 DataBar Expanded Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarexpandedcomposite",
      "cc": "A"
    },
    "example": "(01)90012345678908(3103)001750|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_expanded_ccb",
    "name": "GS1 DataBar Expanded Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarexpandedcomposite",
      "cc": "B"
    },
    "example": "(01)90012345678908(3103)001750|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_limited_cca",
    "name": "GS1 DataBar Limited Composite (CC-A)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarlimitedcomposite",
      "cc": "A"
    },
    "example": "(01)15012345678907|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "composite_databar_limited_ccb",
    "name": "GS1 DataBar Limited Composite (CC-B)",
    "category": "Composite (Linear + 2D)",
    "renderer": "custom",
    "bwippType": null,
    "options": {
      "base": "databarlimitedcomposite",
      "cc": "B"
    },
    "example": "(01)15012345678907|(99)1234567",
    "customHandler": "render_composite",
    "notes": ""
  },
  {
    "id": "ultracode",
    "name": "Ultracode",
    "category": "2D - Matrix",
    "renderer": "bwipp",
    "bwippType": "ultracode",
    "options": {},
    "example": "Hello",
    "customHandler": null,
    "notes": "Colour 2D matrix (6-colour palette: white/cyan/magenta/yellow/green/black). Rendered client-side via the Rust/WASM ColorMatrix path; the SVG/PNG carry per-cell palette fills."
  }
] satisfies CatalogEntry[];

export const catalogById = new Map(catalog.map((entry) => [entry.id, entry]));
