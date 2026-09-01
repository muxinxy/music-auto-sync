Add-Type -AssemblyName System.Drawing

$size = 1024
$bitmap = New-Object System.Drawing.Bitmap($size, $size)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$graphics.Clear([System.Drawing.Color]::Transparent)

$center = $size / 2
$outer = New-Object System.Drawing.RectangleF(112, 112, 800, 800)
$inner = New-Object System.Drawing.RectangleF(190, 190, 644, 644)
$label = New-Object System.Drawing.RectangleF(318, 318, 388, 388)

$discBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    $outer,
    [System.Drawing.Color]::FromArgb(255, 220, 45, 54),
    [System.Drawing.Color]::FromArgb(255, 151, 11, 19),
    45
)
$graphics.FillEllipse($discBrush, $outer)

$ringPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(120, 255, 255, 255), 18)
$graphics.DrawEllipse($ringPen, $inner)

$groovePen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(70, 255, 255, 255), 12)
$graphics.DrawEllipse($groovePen, (New-Object System.Drawing.RectangleF(240, 240, 544, 544)))
$graphics.DrawEllipse($groovePen, (New-Object System.Drawing.RectangleF(270, 270, 484, 484)))

$labelBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 255, 247, 239))
$graphics.FillEllipse($labelBrush, $label)

$notePen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 190, 21, 32), 42)
$notePen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$notePen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$graphics.DrawLine($notePen, 570, 400, 570, 590)
$graphics.DrawLine($notePen, 570, 410, 680, 375)
$noteBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 190, 21, 32))
$graphics.FillEllipse($noteBrush, (New-Object System.Drawing.RectangleF(460, 555, 120, 92)))
$graphics.FillEllipse($noteBrush, (New-Object System.Drawing.RectangleF(630, 515, 120, 92)))

$syncPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 255, 255, 255), 38)
$syncPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$syncPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$graphics.DrawArc($syncPen, (New-Object System.Drawing.RectangleF(64, 64, 896, 896)), 212, 58)
$graphics.DrawArc($syncPen, (New-Object System.Drawing.RectangleF(64, 64, 896, 896)), 32, 58)

$arrowBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)
$topArrow = New-Object System.Drawing.Drawing2D.GraphicsPath
$topArrow.AddPolygon([System.Drawing.PointF[]]@((New-Object System.Drawing.PointF(735, 112)), (New-Object System.Drawing.PointF(842, 116)), (New-Object System.Drawing.PointF(800, 210))))
$graphics.FillPath($arrowBrush, $topArrow)
$bottomArrow = New-Object System.Drawing.Drawing2D.GraphicsPath
$bottomArrow.AddPolygon([System.Drawing.PointF[]]@((New-Object System.Drawing.PointF(289, 912)), (New-Object System.Drawing.PointF(182, 908)), (New-Object System.Drawing.PointF(224, 814))))
$graphics.FillPath($arrowBrush, $bottomArrow)

$output = Join-Path $PSScriptRoot "app-icon.png"
$bitmap.Save($output, [System.Drawing.Imaging.ImageFormat]::Png)

$bottomArrow.Dispose()
$topArrow.Dispose()
$arrowBrush.Dispose()
$syncPen.Dispose()
$noteBrush.Dispose()
$notePen.Dispose()
$groovePen.Dispose()
$ringPen.Dispose()
$labelBrush.Dispose()
$discBrush.Dispose()
$graphics.Dispose()
$bitmap.Dispose()
