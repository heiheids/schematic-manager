#!/usr/bin/env pwsh
# 离线类型检查。
#
# 背景：本机无法访问 crates.io（schannel 被阻断），cargo 拉不到依赖，
# 因此无法用 cargo check。但类型检查本身不需要真实依赖——这里用最小桩替代
# flate2 / serde / serde_json / tauri / ureq，再用 rustc 直接检查源码。
#
# 覆盖：src-tauri/src 下除 commands / lib / main 之外的模块及其单元测试。
#   模块按源码的包结构挂载（见 check.rs / check_cmds.rs），各模块末尾的
#   `#[cfg(test)] #[path = "../tests/…"] mod tests;` 会把 src/tests 下的
#   测试文件一并带进 --test 构建，因此不需要逐文件登记。
# 不覆盖：commands 的宏展开与 serde 的字段重命名行为——check_cmds 只验证
#   剥掉宏属性后的函数体，这部分由 CI 的 cargo 构建覆盖。
#
# 用途：在没有网络、没有 cargo 依赖的情况下，快速发现真实编译错误。
# 注意：这不是完整构建，也不能替代 CI —— 它用桩做类型检查并运行部分测试。
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$cargo = "$env:USERPROFILE\.cargo\bin"
if (Test-Path $cargo) { $env:Path = "$cargo;$env:Path" }
# 本机拒绝对系统临时目录的写入，而部分测试需要临时目录。
# 指到工作区内可绕开该环境限制，且不影响被测代码的行为。
$localTemp = Join-Path (Split-Path (Split-Path $here -Parent) -Parent) '_temp'
New-Item -ItemType Directory -Force -Path $localTemp | Out-Null
$env:TEMP = $localTemp
$env:TMP = $localTemp
$env:CARGO_MANIFEST_DIR = (Resolve-Path "$here\..\..\src-tauri").Path
# 源码里有 env!("CARGO_PKG_VERSION") 这类 cargo 注入的变量，rustc 直接编译时
# 需要自己从 Cargo.toml 读出来补上。
$cargoToml = Get-Content (Join-Path $env:CARGO_MANIFEST_DIR 'Cargo.toml') -Raw
$env:CARGO_PKG_VERSION = [regex]::Match($cargoToml, '(?m)^version = "([^"]+)"').Groups[1].Value
$env:CARGO_PKG_NAME = [regex]::Match($cargoToml, '(?m)^name = "([^"]+)"').Groups[1].Value

$srcDir = (Resolve-Path (Join-Path $here '..\..\src-tauri\src')).Path
$excluded = @('lib.rs', 'main.rs', 'error.rs')

# ---------------------------------------------------------------- 0/6 模块登记
# 源码模块必须被 check.rs / check_cmds.rs 的 #[path] 覆盖（包级挂载按目录展开），
# 否则该模块的测试会静默不执行——表面上「检查通过」，实际一次都没跑。
Write-Host "0/6 核对模块登记…"
function Get-CoveredFiles {
    param([string[]]$Files)
    $set = New-Object 'System.Collections.Generic.HashSet[string]'
    foreach ($f in $Files) {
        if (-not (Test-Path $f)) { continue }
        # 行内模块（mod app { ... }）里的 #[path] 以 tools/typecheck/<模块名>/ 为基准，
        # 其余以 tools/typecheck/ 为基准。
        $inline = $null
        foreach ($line in (Get-Content $f)) {
            if ($line -match '^mod ([a-z_0-9]+) \{\s*$') {
                $inline = $Matches[1]
                continue
            }
            if ($inline -and $line -match '^\}\s*$') {
                $inline = $null
                continue
            }
            $m = [regex]::Match($line, '#\[path = "([^"]+)"\]')
            if (-not $m.Success) { continue }
            $base = if ($inline) { Join-Path $here $inline } else { $here }
            $abs = [System.IO.Path]::GetFullPath((Join-Path $base $m.Groups[1].Value))
            if (Test-Path $abs -PathType Leaf) { [void]$set.Add($abs) }
            if ([System.IO.Path]::GetFileName($abs) -eq 'mod.rs') {
                $dir = Split-Path -Parent $abs
                if (Test-Path $dir) {
                    Get-ChildItem $dir -Filter '*.rs' -File -ErrorAction SilentlyContinue |
                        ForEach-Object { [void]$set.Add($_.FullName) }
                }
            }
        }
    }
    return $set
}
$covered = Get-CoveredFiles @((Join-Path $here 'check.rs'), (Join-Path $here 'check_cmds.rs'))
$missing = @()
Get-ChildItem $srcDir -Recurse -Filter '*.rs' -File | ForEach-Object {
    $rel = $_.FullName.Substring($srcDir.Length).TrimStart([char[]]@('\', '/'))
    $parts = $rel -split '[\\/]'
    if ($parts[0] -eq 'tests') { return }
    if ($_.Name -eq 'mod.rs') { return }
    if ($parts.Count -eq 1 -and $excluded -contains $_.Name) { return }
    if ($parts[0] -eq 'commands') { return }
    if (-not $covered.Contains($_.FullName)) { $missing += $rel }
}
if ($missing.Count -gt 0) {
    Write-Host "以下模块未挂载到 check.rs / check_cmds.rs，其测试不会被执行："
    $missing | ForEach-Object { Write-Host "  $_" }
    Write-Host "请把它们加入对应检查文件的 #[path] 挂载，或在 run.ps1 的 excluded 中说明原因。"
    exit 1
}
Write-Host "  源码模块均已挂载。"

# ---------------------------------------------------------------- 1/6 编译桩
Write-Host "1/6 编译依赖桩…"
$stubs = @(
    @{ name = 'flate2'; src = 'flate2.rs'; out = 'libflate2.rlib' },
    @{ name = 'serde'; src = 'serde_stub_crate.rs'; out = 'libserde.rlib' },
    @{ name = 'serde_json'; src = 'serde_json_stub_crate.rs'; out = 'libserde_json.rlib' },
    @{ name = 'tauri'; src = 'tauri_stub_crate.rs'; out = 'libtauri.rlib' },
    @{ name = 'ureq'; src = 'ureq_stub_crate.rs'; out = 'libureq.rlib' }
)
foreach ($s in $stubs) {
    $out = Join-Path $here $s.out
    Remove-Item $out -Force -ErrorAction SilentlyContinue
    $stubOut = & cmd /c "rustc --edition 2021 --crate-type lib --crate-name $($s.name) --emit=link -o `"$out`" `"$(Join-Path $here $s.src)`" 2>&1"
    if (-not (Test-Path $out)) {
        $stubOut | Select-Object -First 20 | ForEach-Object { Write-Host "    $_" }
        Write-Host "  $($s.name) 桩编译失败"
        exit 1
    }
}
$externArgs = "--extern `"flate2=$(Join-Path $here 'libflate2.rlib')`" " +
    "--extern `"serde=$(Join-Path $here 'libserde.rlib')`" " +
    "--extern `"serde_json=$(Join-Path $here 'libserde_json.rlib')`" " +
    "--extern `"tauri=$(Join-Path $here 'libtauri.rlib')`" " +
    "--extern `"ureq=$(Join-Path $here 'libureq.rlib')`""
Write-Host "  5 个桩就绪。"

# ---------------------------------------------------------------- 2/6 纯逻辑模块
Write-Host "2/6 类型检查纯逻辑模块…"
$theoryOut = & cmd /c "rustc --edition 2021 --crate-type lib --crate-name check --test $externArgs `"$(Join-Path $here 'check.rs')`" --emit=metadata -o `"$(Join-Path $here 'check.rmeta')`" 2>&1"
$theoryOut | Where-Object { $_ -notmatch '^\s*$' -and $_ -notmatch '^warning' -and $_ -notmatch '^\s+-->|^\s+\||^\s+=|^\s*$|^\s*\.\.\.' }
$theoryErr = @($theoryOut | Select-String -Pattern '^error').Count
Write-Host "  类型检查 -> 错误: $theoryErr"
if ($theoryErr -gt 0) { exit $theoryErr }

# ---------------------------------------------------------------- 3/6 命令层
Write-Host "3/6 生成并检查命令层与 DTO…"
$py = "C:\Users\hsds\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\python\python.exe"
if (-not (Test-Path $py)) { $py = "python" }
# 先清掉旧副本：生成中途失败时若不清理，第 3/6 步会编译上一次的旧文件并「通过」。
$generated = Join-Path $here 'generated'
Remove-Item $generated -Recurse -Force -ErrorAction SilentlyContinue
$genOut = & $py (Join-Path $here 'gen_cmd_check.py') 2>&1
$genExit = $LASTEXITCODE
if ($genExit -ne 0) {
    $genOut | Select-Object -Last 20 | ForEach-Object { Write-Host "    $_" }
    Write-Host "  gen_cmd_check.py 退出码 $genExit"
    exit $genExit
}
$genDir = Join-Path $generated 'commands'
$genCount = @(Get-ChildItem $genDir -Filter '*.rs' -File -ErrorAction SilentlyContinue).Count
$srcCount = @(Get-ChildItem (Join-Path $env:CARGO_MANIFEST_DIR 'src\commands') -Filter '*.rs' -File).Count
if ($genCount -ne $srcCount) {
    Write-Host "  命令层副本数量不符：生成 $genCount 个，源码 $srcCount 个"
    exit 1
}
Write-Host "  已生成 $genCount 个命令层副本。"
$cmdsOut = & cmd /c "rustc --edition 2021 --crate-type bin --crate-name cmds $externArgs `"$(Join-Path $here 'check_cmds.rs')`" --emit=metadata -o `"$(Join-Path $here 'cmds.rmeta')`" 2>&1"
$cmdsErr = @($cmdsOut | Select-String -Pattern '^error').Count
if ($cmdsErr -gt 0) { $cmdsOut | Select-Object -First 40 }
Write-Host "  命令层 -> 错误: $cmdsErr"
if ($cmdsErr -gt 0) { exit $cmdsErr }

# ---------------------------------------------------------------- 4/6 命令层测试
# 上一步不带 --test，因此 cfg(test) 里的代码完全没有被检查——而命令层的测试
# 正是覆盖「导入是否生效、DTO 字段是否同步」的地方。
Write-Host "4/6 运行命令层与 DTO 的单元测试…"
$cmdsExe = Join-Path $here 'cmdscheck.exe'
Remove-Item $cmdsExe -Force -ErrorAction SilentlyContinue
$cmdsBuild = & cmd /c "rustc --edition 2021 --crate-type lib --crate-name cmdscheck --test $externArgs `"$(Join-Path $here 'check_cmds.rs')`" -o `"$cmdsExe`" 2>&1"
$cmdsTestErr = @($cmdsBuild | Select-String -Pattern '^error').Count
if ($cmdsTestErr -gt 0 -or -not (Test-Path $cmdsExe)) {
    $cmdsBuild | Select-Object -First 40
    Write-Host "  命令层测试编译失败"
    if ($cmdsTestErr -eq 0) { $cmdsTestErr = 1 }
    exit $cmdsTestErr
}
$cmdsTestOut = & $cmdsExe --test-threads=4 2>&1
$cmdsTestExit = $LASTEXITCODE
# 受桩限制：DTO 的字段名契约靠 serde 的 rename_all 生成，而本地 serde 桩没有
# 过程宏，to_value 只能返回空对象。这些测试在本地必然失败，但必须保留并运行：
# CI 用真实 serde，它们在那里才真正生效；本地能编译它们，就足以发现字段改名
# 后忘记同步断言这类错误。
# 受桩限制的 3 条必须逐条列出，不能按 dto 模块前缀整体豁免：
# 前缀豁免会把该模块今后新增的、不依赖 rename_all 的真实回归也一并吞掉。
$cmdsStubBound = @(
    'commands::dto::tests::backup_entry_created_ns_is_a_string',
    'commands::dto::tests::convertible_keys_are_camel_case',
    'commands::dto::tests::update_info_keys_are_camel_case'
)
$cmdsFailed = $cmdsTestOut | Select-String -Pattern '^    ((?:[a-z_][a-z_0-9]*::)+[a-z_0-9]+)$' |
    ForEach-Object { $_.Matches[0].Groups[1].Value }
$cmdsReal = $cmdsFailed | Where-Object { $cmdsStubBound -notcontains $_ }
if ($cmdsReal.Count -gt 0) {
    Write-Host "  命令层测试失败（与代码相关）："
    $cmdsReal | ForEach-Object { Write-Host "    $_" }
    exit $cmdsReal.Count
}
if ($cmdsTestExit -ne 0 -and $cmdsFailed.Count -ne $cmdsStubBound.Count) {
    Write-Host "  命令层测试退出码 $cmdsTestExit，失败 $($cmdsFailed.Count) 项，与已知受桩限制的 $($cmdsStubBound.Count) 项不符：$($cmdsFailed -join ', ')"
    $cmdsTestOut | Select-Object -Last 20 | ForEach-Object { Write-Host "    $_" }
    exit $cmdsTestExit
}
$cmdsTotal = $cmdsTestOut | Select-String -Pattern 'test result: (ok|FAILED)\. (\d+) passed'
if ($cmdsTotal) {
    $n = $cmdsTotal.Matches[0].Groups[2].Value
    Write-Host "  命令层与 DTO -> $n 项通过，$($cmdsFailed.Count) 项受本地 serde 桩限制（CI 覆盖）"
}

# ---------------------------------------------------------------- 5/6 纯逻辑测试
Write-Host "5/6 运行单元测试…"
$exe = Join-Path $here 'check.exe'
Remove-Item $exe -Force -ErrorAction SilentlyContinue
$exeBuild = & cmd /c "rustc --edition 2021 --crate-type lib --crate-name check --test $externArgs `"$(Join-Path $here 'check.rs')`" -o `"$exe`" 2>&1"
if (-not (Test-Path $exe)) {
    $exeBuild | Select-Object -First 30 | ForEach-Object { Write-Host "    $_" }
    Write-Host "  测试可执行文件编译失败"
    exit 1
}
$testOut = & $exe --test-threads=4 2>&1
$testExit = $LASTEXITCODE
$testOut | Where-Object { $_ -match 'test result|^failures:|^    ' } | Select-Object -First 40

# 退出码以测试可执行文件自身的返回值为准。若二进制崩溃或提前 abort，
# 输出里没有可匹配的失败项，只看输出会把彻底失败说成成功。
if ($testExit -eq 0) {
    Write-Host ""
    Write-Host "测试全部通过。"
} else {
    # 已知受桩限制的测试（真实 gzip/zlib 相关）。本机 flate2 桩已实现解压，
    # 该清单目前为空；出现桩局限时把具体测试全名加进来，不要按模块前缀过滤。
    $stubBound = @()
    $names = $testOut | Select-String -Pattern '^    ((?:[a-z_][a-z_0-9]*::)+[a-z_0-9]+)$' |
        ForEach-Object { $_.Matches[0].Groups[1].Value }
    $real = $names | Where-Object { $stubBound -notcontains $_ }
    Write-Host ""
    Write-Host "失败项中与代码相关的:"
    if ($real.Count -eq 0) {
        Write-Host "  未能从输出中定位失败项（测试进程退出码 $testExit；可能是崩溃或超时）"
        exit $testExit
    }
    $real | ForEach-Object { Write-Host "  $_" }
    exit $real.Count
}

# ---------------------------------------------------------------- 附加：演示程序
# 它用真实数据走完整条链路，是人工查看进展的主要入口。这里做一次类型检查，
# 成本只有几百毫秒；它引用的模块一旦更名会立刻暴露。
Write-Host ""
Write-Host "附加：类型检查进展演示程序…"
$demoOut = & cmd /c "rustc --edition 2021 --crate-type bin --crate-name demo $externArgs `"$(Join-Path $here 'demo.rs')`" --emit=metadata -o `"$(Join-Path $here 'demo.rmeta')`" 2>&1"
$demoErr = @($demoOut | Select-String -Pattern '^error').Count
if ($demoErr -gt 0) {
    $demoOut | Select-Object -First 30
    Write-Host "  进展演示程序类型检查失败"
    exit $demoErr
}
Write-Host "  进展演示程序 -> 错误: 0"
exit 0
