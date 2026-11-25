#!/usr/bin/env python3
"""
Generates the 10 deterministic synthetic PDF fixtures required for Phase 9 testing:
1. digital_text.pdf
2. ligatures_ot1.pdf
3. two_column.pdf
4. rotated_cropbox.pdf
5. corrupt_cmap.pdf
6. corrupt_xref.pdf
7. encrypted.pdf
8. table_formula.pdf
9. xss_payload.pdf
10. dates_and_orders.pdf
"""

import os
from pathlib import Path

def assemble_pdf(objects, trailer_extra=""):
    """Assemble a standard cross-reference indexed PDF."""
    out = b"%PDF-1.5\n"
    offsets = []
    for i, obj in enumerate(objects, 1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n{obj}\nendobj\n".encode("latin1")
    
    xref_offset = len(out)
    out += b"xref\n"
    out += f"0 {len(objects)+1}\n".encode("latin1")
    out += b"0000000000 65535 f \n"
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode("latin1")
    
    out += f"trailer\n<< /Size {len(objects)+1} /Root 1 0 R {trailer_extra} >>\nstartxref\n{xref_offset}\n%%EOF\n".encode("latin1")
    return out

def build_single_page_pdf(content_stream, font_extra="", page_extra="", trailer_extra="", extra_objs=None):
    if extra_objs is None:
        extra_objs = []
    
    objs = []
    # 1: Catalog
    objs.append("<< /Type /Catalog /Pages 2 0 R >>")
    # 2: Pages
    objs.append("<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    # 3: Page
    objs.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] {page_extra} /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>")
    # 4: Contents
    objs.append(f"<< /Length {len(content_stream)} >>\nstream\n{content_stream}\nendstream")
    # 5: Font
    objs.append(f"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica {font_extra} >>")
    
    for eo in extra_objs:
        objs.append(eo)
        
    return assemble_pdf(objs, trailer_extra)

def main():
    fixtures_dir = Path("tests/fixtures")
    fixtures_dir.mkdir(parents=True, exist_ok=True)

    # 1. digital_text.pdf (2 pages)
    p1_content = "BT /F1 12 Tf 72 750 Td (Application For Office Use Only.) Tj 72 720 Td (Connected Load: 5kW.) Tj 72 690 Td (Single column clean digital text for pdfmd-lite conversion pipeline.) Tj ET"
    p2_content = "BT /F1 12 Tf 72 750 Td (Sub Registrar Office, Pattom.) Tj 72 720 Td (Registration details and statutory legal conveyance notes.) Tj ET"
    objs = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 5 0 R /Resources << /Font << /F1 7 0 R >> >> >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 6 0 R /Resources << /Font << /F1 7 0 R >> >> >>",
        f"<< /Length {len(p1_content)} >>\nstream\n{p1_content}\nendstream",
        f"<< /Length {len(p2_content)} >>\nstream\n{p2_content}\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    (fixtures_dir / "digital_text.pdf").write_bytes(assemble_pdf(objs))

    # 2. ligatures_ot1.pdf
    content_lig = "BT /F1 12 Tf 72 750 Td (The official definition specifies sufficient flight affiliation for floating traffic.) Tj 72 720 Td (Standard ligatures: fi, fl, ff, ffi, ffl and OT1 accents.) Tj ET"
    (fixtures_dir / "ligatures_ot1.pdf").write_bytes(build_single_page_pdf(content_lig))

    # 3. two_column.pdf
    content_2col = (
        "BT /F1 10 Tf "
        "72 750 Td (Left Column: Paragraph 1 begins here with introductory concepts.) Tj "
        "72 730 Td (Left Column: Paragraph 2 continues discussing the methodology.) Tj "
        "72 710 Td (Left Column: Paragraph 3 concludes the first stage results.) Tj "
        "300 750 Td (Right Column: Section 2 begins with analytical comparisons.) Tj "
        "300 730 Td (Right Column: Empirical evaluation shows strong convergence.) Tj "
        "300 710 Td (Right Column: Further discussion on boundary conditions.) Tj "
        "ET"
    )
    (fixtures_dir / "two_column.pdf").write_bytes(build_single_page_pdf(content_2col))

    # 4. rotated_cropbox.pdf
    content_rot = "BT /F1 12 Tf 72 750 Td (Rotated page with non-zero CropBox boundary.) Tj ET"
    (fixtures_dir / "rotated_cropbox.pdf").write_bytes(
        build_single_page_pdf(content_rot, page_extra="/CropBox [36 36 559 806] /Rotate 90")
    )

    # 5. corrupt_cmap.pdf
    # /ToUnicode CMap mapping :DWFK -> Watch
    cmap_str = (
        "/CIDInit /ProcSet findresource begin\n"
        "12 dict begin\n"
        "begincmap\n"
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
        "/CMapName /CustomToUnicode def\n"
        "/CMapType 2 def\n"
        "1 begincodespacerange\n"
        "<00> <FF>\n"
        "endcodespacerange\n"
        "5 beginbfchar\n"
        "<3A> <0057>\n"  # ':' -> 'W'
        "<44> <0061>\n"  # 'D' -> 'a'
        "<57> <0074>\n"  # 'W' -> 't'
        "<46> <0063>\n"  # 'F' -> 'c'
        "<4B> <0068>\n"  # 'K' -> 'h'
        "endbfchar\n"
        "endcmap\n"
        "CMapName currentdict /CMap defineresource pop\n"
        "end\n"
        "end\n"
    )
    cmap_obj = f"<< /Length {len(cmap_str)} >>\nstream\n{cmap_str}\nendstream"
    content_cmap = "BT /F1 12 Tf 72 750 Td (:DWFK) Tj /F2 12 Tf ( and Four Levels of Measurement) Tj ET"
    objs_cmap = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>",
        f"<< /Length {len(content_cmap)} >>\nstream\n{content_cmap}\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 7 0 R >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        cmap_obj,
    ]
    (fixtures_dir / "corrupt_cmap.pdf").write_bytes(assemble_pdf(objs_cmap))

    # 6. corrupt_xref.pdf
    valid_pdf = build_single_page_pdf("BT /F1 12 Tf 72 750 Td (Corrupted xref test) Tj ET")
    # Corrupt the xref table header
    corrupt_pdf = valid_pdf.replace(b"0000000000 65535 f", b"INVALID_CORRUPT_XREF_BYTES!")
    (fixtures_dir / "corrupt_xref.pdf").write_bytes(corrupt_pdf)

    # 7. encrypted.pdf
    enc_pdf = build_single_page_pdf(
        "BT /F1 12 Tf 72 750 Td (Password protected encrypted content) Tj ET",
        trailer_extra="/Encrypt << /Filter /Standard /V 2 /R 3 /P -4 /O (12345678901234567890123456789012) /U (12345678901234567890123456789012) >>"
    )
    (fixtures_dir / "encrypted.pdf").write_bytes(enc_pdf)

    # 8. table_formula.pdf
    content_tf = (
        "BT /F1 12 Tf 72 750 Td (Table 1: Synthetic Experimental Results) Tj ET\n"
        "BT /F1 10 Tf "
        "72 720 Td (| Item | Quantity | Price |) Tj "
        "72 700 Td (| Alpha | 10 | $100 |) Tj "
        "72 680 Td (| Beta | 20 | $200 |) Tj "
        "ET\n"
        "BT /F1 12 Tf 72 640 Td (Given equation: $x^2 + y^2 = z^2$ and display $$E = mc^2$$) Tj ET"
    )
    (fixtures_dir / "table_formula.pdf").write_bytes(build_single_page_pdf(content_tf))

    # 9. xss_payload.pdf
    content_xss = (
        "BT /F1 10 Tf "
        "72 750 Td (<script>alert(1)</script>) Tj "
        "72 730 Td (javascript:alert(document.cookie)) Tj "
        "72 710 Td (<img src=x onerror=alert(1)>) Tj "
        "ET"
    )
    annot_obj = "<< /Type /Annot /Subtype /Link /Rect [72 720 200 740] /A << /S /URI /URI (javascript:alert(1)) >> >>"
    (fixtures_dir / "xss_payload.pdf").write_bytes(
        build_single_page_pdf(content_xss, page_extra="/Annots [6 0 R]", extra_objs=[annot_obj])
    )

    # 10. dates_and_orders.pdf
    content_dates = (
        "BT /F1 11 Tf "
        "72 750 Td (GOVERNMENT OF KERALA) Tj "
        "72 730 Td (G.O. (Ms) No. 123/2024/HEDN Dated 15.03.2024) Tj "
        "72 710 Td (Order No. 456-B dated 2023-11-20 regarding administrative approval.) Tj "
        "72 680 Td (1. The committee met on 10.01.2024 to deliberate on terms.) Tj "
        "72 660 Td (2. Resolved that procedures outlined in Section IV be enforced.) Tj "
        "72 640 Td (3. Final sanctioned budget allocation of Rs. 50,00,000.) Tj "
        "ET"
    )
    (fixtures_dir / "dates_and_orders.pdf").write_bytes(build_single_page_pdf(content_dates))

    print(f"Successfully generated all 10 synthetic fixtures in {fixtures_dir.resolve()}")

if __name__ == "__main__":
    main()
