//! Deterministic ECMA-376 OOXML fixture builders for PivotTables, VBA `.xlsm`, Slicers, and Complex Charts.

use crate::error::XlsxError;
use crate::merge::merge_engine_with_preserved;
use crate::preserve::{
    PivotCacheEntry, PreservedPackage, PreservedPart, RawRelationship, SheetAttachments,
};
use ordia_sheet_engine::{CellCoord, IronCalcEngine, SheetEngine as _};
use std::collections::BTreeMap;

fn base_sales_engine(title: &str) -> Result<Vec<u8>, XlsxError> {
    let mut engine = IronCalcEngine::new_empty(title)?;
    engine.set_cell_input(0, CellCoord::new(0, 0), "Region")?;
    engine.set_cell_input(0, CellCoord::new(0, 1), "Quarter")?;
    engine.set_cell_input(0, CellCoord::new(0, 2), "Revenue")?;
    let rows = [
        ("华东", "Q1", "1200"),
        ("华南", "Q1", "950"),
        ("华东", "Q2", "1450"),
        ("华北", "Q2", "1100"),
    ];
    for (idx, (reg, qtr, rev)) in rows.iter().enumerate() {
        let r = (idx + 1) as u32;
        engine.set_cell_input(0, CellCoord::new(r, 0), reg)?;
        engine.set_cell_input(0, CellCoord::new(r, 1), qtr)?;
        engine.set_cell_input(0, CellCoord::new(r, 2), rev)?;
    }
    engine.set_cell_input(0, CellCoord::new(5, 0), "Total")?;
    engine.set_cell_input(0, CellCoord::new(5, 2), "=SUM(C2:C5)")?;
    Ok(engine.to_xlsx_bytes()?)
}

/// Builds a synthetic ECMA-376 `.xlsx` containing a PivotCache and PivotTable referencing `Sheet1!A1:C5`.
pub fn build_pivot_table_xlsx() -> Result<Vec<u8>, XlsxError> {
    let base = base_sales_engine("PivotSample")?;
    let mut pkg = PreservedPackage::default();

    let cache_def = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:id="rId1" recordCount="4">
  <cacheSource type="worksheet"><worksheetSource ref="A1:C5" sheet="Sheet1"/></cacheSource>
  <cacheFields count="3">
    <cacheField name="Region" numFmtId="0"><sharedItems><s v="华东"/><s v="华南"/><s v="华北"/></sharedItems></cacheField>
    <cacheField name="Quarter" numFmtId="0"><sharedItems><s v="Q1"/><s v="Q2"/></sharedItems></cacheField>
    <cacheField name="Revenue" numFmtId="0"><sharedItems containsNumber="1" minValue="950" maxValue="1450"/></sharedItems></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#.as_bytes();

    let cache_records = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheRecords xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="4">
  <r><x v="0"/><x v="0"/><n v="1200"/></r>
  <r><x v="1"/><x v="0"/><n v="950"/></r>
  <r><x v="0"/><x v="1"/><n v="1450"/></r>
  <r><x v="2"/><x v="1"/><n v="1100"/></r>
</pivotCacheRecords>"#;

    let cache_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheRecords" Target="pivotCacheRecords1.xml"/>
</Relationships>"#;

    let pivot_table = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="SalesPivot1" cacheId="1" dataCaption="Values">
  <location ref="E1:G5" firstHeaderRow="1" firstDataRow="2" firstDataCol="1"/>
  <pivotFields count="3">
    <pivotField axis="axisRow" showAll="0"><items count="4"><item x="0"/><item x="1"/><item x="2"/><item t="default"/></items></pivotField>
    <pivotField showAll="0"/>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <dataFields count="1"><dataField name="Sum of Revenue" fld="2" baseField="0" baseItem="0"/></dataFields>
</pivotTableDefinition>"#;

    let pivot_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition1.xml"/>
</Relationships>"#;

    for (path, data) in [
        ("xl/pivotCache/pivotCacheDefinition1.xml", cache_def),
        (
            "xl/pivotCache/pivotCacheRecords1.xml",
            cache_records.as_slice(),
        ),
        (
            "xl/pivotCache/_rels/pivotCacheDefinition1.xml.rels",
            cache_rels.as_slice(),
        ),
        ("xl/pivotTables/pivotTable1.xml", pivot_table.as_slice()),
        (
            "xl/pivotTables/_rels/pivotTable1.xml.rels",
            pivot_rels.as_slice(),
        ),
    ] {
        pkg.unknown_parts.insert(
            path.to_string(),
            PreservedPart {
                bytes: data.to_vec(),
                stored_uncompressed: false,
            },
        );
    }

    pkg.extra_content_type_overrides = vec![
        (
            "/xl/pivotCache/pivotCacheDefinition1.xml".into(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml"
                .into(),
        ),
        (
            "/xl/pivotCache/pivotCacheRecords1.xml".into(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml"
                .into(),
        ),
        (
            "/xl/pivotTables/pivotTable1.xml".into(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml".into(),
        ),
    ];

    pkg.extra_workbook_rels.push(RawRelationship {
        orig_id: "rId90".into(),
        rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition".into(),
        target: "pivotCache/pivotCacheDefinition1.xml".into(),
        target_mode: None,
    });
    pkg.pivot_caches.push(PivotCacheEntry {
        cache_id: "1".into(),
        orig_rid: "rId90".into(),
        definition_part: "xl/pivotCache/pivotCacheDefinition1.xml".into(),
        source_sheet: Some("Sheet1".into()),
        source_ref: Some("A1:C5".into()),
    });

    pkg.sheet_attachments.insert(
        1,
        SheetAttachments {
            orig_name: "Sheet1".into(),
            extra_rels: vec![RawRelationship {
                orig_id: "rId80".into(),
                rel_type:
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable"
                        .into(),
                target: "../pivotTables/pivotTable1.xml".into(),
                target_mode: None,
            }],
            ..Default::default()
        },
    );

    merge_engine_with_preserved(&base, &pkg)
}

/// Builds a synthetic macro-enabled `.xlsm` containing a binary `xl/vbaProject.bin` stream.
pub fn build_macro_vba_xlsm() -> Result<Vec<u8>, XlsxError> {
    let base = base_sales_engine("MacroSample")?;
    let mut pkg = PreservedPackage {
        workbook_content_type: Some("application/vnd.ms-excel.sheet.macroEnabled.main+xml".into()),
        workbook_pr_xml: Some(r#"<workbookPr codeName="ThisWorkbook"/>"#.into()),
        extra_content_type_defaults: vec![(
            "bin".into(),
            "application/vnd.ms-office.vbaProject".into(),
        )],
        ..Default::default()
    };

    // Deterministic OLE2 Compound File header magic + synthetic VBA module stream bytes (512 bytes).
    let mut vba_bin = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    vba_bin.extend_from_slice(
        b"Attribute VB_Name = \"OrdiaMacroModule\"\r\nSub RecalcSummary()\r\nEnd Sub\r\n",
    );
    while vba_bin.len() < 512 {
        vba_bin.push((vba_bin.len() as u8).wrapping_mul(31).wrapping_add(7));
    }

    pkg.unknown_parts.insert(
        "xl/vbaProject.bin".into(),
        PreservedPart {
            bytes: vba_bin,
            stored_uncompressed: true,
        },
    );
    pkg.extra_workbook_rels.push(RawRelationship {
        orig_id: "rId91".into(),
        rel_type: "http://schemas.microsoft.com/office/2006/relationships/vbaProject".into(),
        target: "vbaProject.bin".into(),
        target_mode: None,
    });

    merge_engine_with_preserved(&base, &pkg)
}

/// Builds a synthetic `.xlsx` containing SlicerCache + Slicer parts and `<extLst>` `r:id` bindings.
pub fn build_slicer_xlsx() -> Result<Vec<u8>, XlsxError> {
    let base = base_sales_engine("SlicerSample")?;
    let mut pkg = PreservedPackage::default();

    let slicer_cache = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<slicerCacheDefinition xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" name="Slicer_Region" sourceName="Region">
  <data><tabular name="Region"><items count="3"><i n="华东"/><i n="华南"/><i n="华北"/></items></tabular></data>
</slicerCacheDefinition>"#.as_bytes();

    let slicer_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<slicers xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main">
  <slicer name="Region 1" cache="Slicer_Region" caption="地区筛选" columnCount="1" rowHeight="228600"/>
</slicers>"#.as_bytes();

    for (path, data) in [
        ("xl/slicerCaches/slicerCache1.xml", slicer_cache),
        ("xl/slicers/slicer1.xml", slicer_xml),
    ] {
        pkg.unknown_parts.insert(
            path.into(),
            PreservedPart {
                bytes: data.to_vec(),
                stored_uncompressed: false,
            },
        );
    }

    pkg.extra_content_type_overrides = vec![
        (
            "/xl/slicerCaches/slicerCache1.xml".into(),
            "application/vnd.ms-excel.slicerCache+xml".into(),
        ),
        (
            "/xl/slicers/slicer1.xml".into(),
            "application/vnd.ms-excel.slicer+xml".into(),
        ),
    ];

    pkg.extra_workbook_rels.push(RawRelationship {
        orig_id: "rId92".into(),
        rel_type: "http://schemas.microsoft.com/office/2007/relationships/slicerCache".into(),
        target: "slicerCaches/slicerCache1.xml".into(),
        target_mode: None,
    });
    pkg.workbook_ext_blocks.push(
        r#"<ext uri="{B58B0392-4F1F-4190-BB64-5DF3571DCE5F}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:slicerCaches><x14:slicerCache r:id="rId92"/></x14:slicerCaches></ext>"#.into(),
    );

    pkg.sheet_attachments.insert(
        1,
        SheetAttachments {
            orig_name: "Sheet1".into(),
            extra_rels: vec![RawRelationship {
                orig_id: "rId82".into(),
                rel_type: "http://schemas.microsoft.com/office/2007/relationships/slicer".into(),
                target: "../slicers/slicer1.xml".into(),
                target_mode: None,
            }],
            preserved_ext_blocks: vec![
                r#"<ext uri="{A8765BA9-456A-4DAB-B4F3-ACF838C121DE}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:slicerList><x14:slicer r:id="rId82"/></x14:slicerList></ext>"#.into(),
            ],
            ..Default::default()
        },
    );

    merge_engine_with_preserved(&base, &pkg)
}

/// Builds a synthetic `.xlsx` containing a secondary-axis combo Chart + Drawing + ChartStyle/Color parts.
pub fn build_complex_chart_xlsx() -> Result<Vec<u8>, XlsxError> {
    let base = base_sales_engine("ComplexChartSample")?;
    let mut pkg = PreservedPackage::default();

    let drawing_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <xdr:twoCellAnchor>
    <xdr:from><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
    <xdr:to><xdr:col>11</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>16</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
    <xdr:graphicFrame macro="">
      <xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="ComboChart 1"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>
      <xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm>
      <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/></a:graphicData></a:graphic>
    </xdr:graphicFrame>
    <xdr:clientData/>
  </xdr:twoCellAnchor>
</xdr:wsDr>"#;

    let drawing_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/>
</Relationships>"#;

    let chart_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <c:chart>
    <c:plotArea>
      <c:barChart>
        <c:barDir val="col"/><c:grouping val="clustered"/>
        <c:ser><c:idx val="0"/><c:order val="0"/><c:cat><c:strRef><c:f>Sheet1!$A$2:$A$5</c:f></c:strRef></c:cat><c:val><c:numRef><c:f>Sheet1!$C$2:$C$5</c:f></c:numRef></c:val></c:ser>
        <c:axId val="101"/><c:axId val="102"/>
      </c:barChart>
      <c:radarChart>
        <c:radarStyle val="marker"/>
        <c:ser><c:idx val="1"/><c:order val="1"/><c:val><c:numRef><c:f>Sheet1!$C$2:$C$5</c:f></c:numRef></c:val></c:ser>
        <c:axId val="201"/><c:axId val="202"/>
      </c:radarChart>
    </c:plotArea>
  </c:chart>
</c:chartSpace>"#;

    let chart_style = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cs:chartStyle xmlns:cs="http://schemas.microsoft.com/office/drawing/2012/chartStyle" id="201"/>"#;

    let chart_colors = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cs:colorStyle xmlns:cs="http://schemas.microsoft.com/office/drawing/2012/chartStyle" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" meth="cycle" id="10"><a:srgbClr val="2563EB"/></cs:colorStyle>"#;

    let chart_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.microsoft.com/office/2011/relationships/chartStyle" Target="style1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.microsoft.com/office/2011/relationships/chartColorStyle" Target="colors1.xml"/>
</Relationships>"#;

    for (path, data) in [
        ("xl/drawings/drawing1.xml", drawing_xml.as_slice()),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            drawing_rels.as_slice(),
        ),
        ("xl/charts/chart1.xml", chart_xml.as_slice()),
        ("xl/charts/style1.xml", chart_style.as_slice()),
        ("xl/charts/colors1.xml", chart_colors.as_slice()),
        ("xl/charts/_rels/chart1.xml.rels", chart_rels.as_slice()),
    ] {
        pkg.unknown_parts.insert(
            path.into(),
            PreservedPart {
                bytes: data.to_vec(),
                stored_uncompressed: false,
            },
        );
    }

    pkg.extra_content_type_overrides = vec![
        (
            "/xl/drawings/drawing1.xml".into(),
            "application/vnd.openxmlformats-officedocument.drawing+xml".into(),
        ),
        (
            "/xl/charts/chart1.xml".into(),
            "application/vnd.openxmlformats-officedocument.drawingml.chart+xml".into(),
        ),
        (
            "/xl/charts/style1.xml".into(),
            "application/vnd.ms-office.chartstyle+xml".into(),
        ),
        (
            "/xl/charts/colors1.xml".into(),
            "application/vnd.ms-office.chartcolorstyle+xml".into(),
        ),
    ];

    pkg.sheet_attachments.insert(
        1,
        SheetAttachments {
            orig_name: "Sheet1".into(),
            extra_rels: vec![RawRelationship {
                orig_id: "rId83".into(),
                rel_type:
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing"
                        .into(),
                target: "../drawings/drawing1.xml".into(),
                target_mode: None,
            }],
            drawing_orig_rid: Some("rId83".into()),
            ..Default::default()
        },
    );

    merge_engine_with_preserved(&base, &pkg)
}

/// Reads all non-engine-managed part payloads from an `.xlsx` / `.xlsm` byte slice for byte-identity assertions.
pub fn extract_unknown_part_bytes(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, XlsxError> {
    let pkg = PreservedPackage::from_zip_bytes(bytes)?;
    Ok(pkg
        .unknown_parts
        .into_iter()
        .map(|(k, v)| (k, v.bytes))
        .collect())
}
