# `summary.json` 字段表

## 顶层字段

| 顺序 | 字段 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `activePaginationEnabled` | 布尔值 | 是否启用球/杆头 PRC 主动分页补取。 |
| 2 | `ballPrcPageCount` | 整数 | 累计收到的球 PRC 页对象数量，包含可能重叠的主动推送页和补取页。 |
| 3 | `ballPrcPointCount` | 整数 | 去重后的球 PRC 点数。 |
| 4 | `cameraBallPointCount` | 整数 | 去重后的相机球点数。 |
| 5 | `cameraClubPointCount` | 整数 | 去重后的相机杆头点数。 |
| 6 | `cameraFailed` | 布尔值 | 相机流程是否失败或被跳过。 |
| 7 | `cameraReferencePointCount` | 整数 | 去重后的相机参考点数。 |
| 8 | `cameraResultReceived` | 布尔值 | 是否收到当前 GUID 的 GVP RESULT。 |
| 9 | `cameraTimedOut` | 布尔值 | 等待 GVP RESULT 是否超时。 |
| 10 | `cameraTriggerSent` | 布尔值 | 是否成功向 GVP 发送击球触发消息。 |
| 11 | `club` | 对象或 `null` | Mevo 杆头高层结果。 |
| 12 | `clubPrcPageCount` | 整数 | 累计收到的杆头 PRC 页数量。 |
| 13 | `clubPrcPointCount` | 整数 | 去重后的杆头 PRC 点数。 |
| 14 | `duplicateBallPrcPoints` | 整数 | 收到的球点总出现次数减去唯一球点数。 |
| 15 | `duplicateClubPrcPoints` | 整数 | 收到的杆头点总出现次数减去唯一杆头点数。 |
| 16 | `expectedBallPrcPointCount` | 整数 | TrackingStatus 或分页状态声明的期望球点数。 |
| 17 | `expectedClubPrcPointCount` | 整数 | ClubResult 或分页状态声明的期望杆头点数。 |
| 18 | `flight` | 对象或 `null` | Mevo 最终球路结果。 |
| 19 | `flightV1` | 对象或 `null` | Mevo 较早到达的阶段性球路结果。 |
| 20 | `guid` | 字符串 | 一次击球在消息流中的唯一关联标识。 |
| 21 | `pagination` | 对象 | 主动分页请求的完成、超时和计数状态。 |
| 22 | `radarComplete` | 布尔值 | 生命周期、飞行结果、点数和分页均满足要求时为真。 |
| 23 | `rawGvpJsonCaptured` | 布尔值 | 是否保存当前 GUID 的原始 GVP RESULT JSON。 |
| 24 | `schemaVersion` | 整数 | Summary 文件结构版本。 |
| 25 | `shotId` | 整数 | 所属 Session 内的击球编号。 |
| 26 | `shotLifecycleComplete` | 布尔值 | 是否已经收到并处理击球生命周期结束事件。 |
| 27 | `speedProfile` | 对象或 `null` | 杆头撞击前后速度曲线。 |
| 28 | `spin` | 对象或 `null` | 旋转检测和候选结果。 |
| 29 | `triggerEpoch` | Unix 秒 | 击球触发消息被记录时的系统时间。 |
| 30 | `warnings` | 字符串数组 | 根据缺失、超时和数量检查生成的质量告警。 |

## `summary.json.club`

| 顺序 | 字段 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `attackAngleDeg` | 度 | 杆头攻击角。 |
| 2 | `clubAzimuthDeg` | 度 | 杆头轨迹方位角。 |
| 3 | `clubElevationDeg` | 度 | 杆头轨迹仰角。 |
| 4 | `clubHeightM` | m | 杆头高度。 |
| 5 | `clubOffsetM` | m | 杆头相对位置偏移。 |
| 6 | `clubToBallTimeMs` | ms | Mevo 内部杆头事件到球事件的时间量。 |
| 7 | `dispersionCorrection` | 数值 | Mevo 内部方向/离散修正参数。 |
| 8 | `dynamicLoftDeg` | 度 | 撞击时动态杆面倾角。 |
| 9 | `faceAngleDeg` | 度 | 撞击时杆面方向角。 |
| 10 | `flags` | 位标志 | 杆头结果原始状态位。 |
| 11 | `numClubPrcPoints` | 整数 | Mevo 声明的杆头 PRC 点数。 |
| 12 | `polyCoeffs` | `12 × 3` 数组 | 杆头拟合系数；顺序为 `Pre_v, Pst_v, Pre_x, Pst_x, Pre_y, Pst_y, Pre_z, Pst_z, Pre_YX, Pst_YX, Pre_ZX, Pst_ZX`。 |
| 13 | `polyScale` | 整数 | 线路中的杆头多项式比例；JSON 系数已完成除法换算。 |
| 14 | `postClubSpeedMps` | m/s | 撞击后杆头速度。 |
| 15 | `postImpactTimeMs` | ms | 撞击后拟合时间边界。 |
| 16 | `preClubSpeedMps` | m/s | 撞击前杆头速度。 |
| 17 | `preImpactTimeMs` | ms | 撞击前拟合时间边界，通常为负值。 |
| 18 | `smashFactor` | 比值 | 球速与撞击前杆速之比。 |
| 19 | `strikeDirectionDeg` | 度 | 杆头击打/杆路方向。 |
| 20 | `swingPlaneHorizontalDeg` | 度 | 挥杆平面水平方向参数。 |
| 21 | `swingPlaneVerticalDeg` | 度 | 挥杆平面竖直倾角。 |

## `summary.json.flight`

| 顺序 | 字段 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `backspinRpm` | rpm | 最终倒旋。 |
| 2 | `carryDistanceM` | m | 第一次落地前的水平 Carry。 |
| 3 | `clubAttackAngleDeg` | 度 | D4 中附带的杆头攻击角。 |
| 4 | `clubEffectiveLoftDeg` | 度 | D4 中附带的有效动态 Loft。 |
| 5 | `clubFaceAngleDeg` | 度 | D4 中附带的杆面角。 |
| 6 | `clubStrikeDirectionDeg` | 度 | D4 中附带的杆路方向。 |
| 7 | `clubSwingPlaneRotationDeg` | 度 | 挥杆平面旋转角。 |
| 8 | `clubSwingPlaneTiltDeg` | 度 | 挥杆平面倾斜角。 |
| 9 | `clubheadSpeedMps` | m/s | 撞击前杆头速度。 |
| 10 | `clubheadSpeedPostMps` | m/s | 撞击后杆头速度。 |
| 11 | `flightTimeSeconds` | 秒 | 模型从起飞到首次落地的总飞行时间。 |
| 12 | `landingPositionM` | `[X,Y,Z]` m | 落点坐标：前进、竖直、侧向。 |
| 13 | `landingSpinRpm` | 三分量 rpm | 球落地时的三轴旋转向量。 |
| 14 | `landingVelocityMps` | `[X,Y,Z]` m/s | 落地瞬间的前进、竖直和侧向速度。 |
| 15 | `launchAzimuthDeg` | 度 | 最终水平起飞方向角。 |
| 16 | `launchElevationDeg` | 度 | 最终垂直起飞角/VLA。 |
| 17 | `launchSpeedMps` | m/s | 最终三维起飞总球速。 |
| 18 | `maximumHeightM` | m | 轨迹最高点。 |
| 19 | `polyScale` | 整数 | 线路中的轨迹系数比例；JSON 系数已完成除法换算。 |
| 20 | `polyX` | 5 项数组 | X/前进位置的 `t⁰～t⁴` 多项式系数。 |
| 21 | `polyY` | 5 项数组 | Y/竖直位置的 `t⁰～t⁴` 多项式系数。 |
| 22 | `polyZ` | 5 项数组 | Z/侧向位置的 `t⁰～t⁴` 多项式系数。 |
| 23 | `riflespinRpm` | rpm | 绕飞行方向轴线的旋转分量。 |
| 24 | `shotCounter` | 整数 | Mevo 设备内部击球计数。 |
| 25 | `sidespinRpm` | rpm | 最终侧旋。 |
| 26 | `startPositionM` | `[X,Y,Z]` m | 模型轨迹起点：前进、竖直、侧向。 |
| 27 | `trackTimeSeconds` | 秒 | 雷达实际跟踪球的时间。 |

## `summary.json.flightV1`

| 顺序 | 字段 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `acceleration` | 算法参数 | 飞行模型使用的加速度相关参数。 |
| 2 | `azimuthDeg` | 度 | 早期水平起飞角。 |
| 3 | `backspinRpm` | rpm | 早期倒旋估计。 |
| 4 | `ballVelocityMps` | m/s | 早期三维球速。 |
| 5 | `clubStrikeDirectionDeg` | 度 | 早期杆路方向字段。 |
| 6 | `clubVelocityMps` | m/s | 早期杆头速度字段。 |
| 7 | `distanceM` | m | 早期预测距离。 |
| 8 | `drag` | 算法参数 | 飞行模型使用的空气阻力相关参数。 |
| 9 | `elevationDeg` | 度 | 早期垂直起飞角。 |
| 10 | `flightTimeSeconds` | 秒 | 早期预测飞行时间。 |
| 11 | `heightM` | m | 早期预测最高点。 |
| 12 | `lateralM` | m | 早期预测侧向落点。 |
| 13 | `polyScale` | 整数 | E8 轨迹系数比例；JSON 系数已完成除法换算。 |
| 14 | `polyX` | 5 项数组 | E8 X 轨迹多项式系数。 |
| 15 | `polyY` | 5 项数组 | E8 Y 轨迹多项式系数。 |
| 16 | `polyZ` | 5 项数组 | E8 Z 轨迹多项式系数。 |
| 17 | `shotCounter` | 整数 | Mevo 设备内部击球计数。 |
| 18 | `sidespinRpm` | rpm | 早期侧旋字段。 |
| 19 | `trackedTimeSeconds` | 秒 | E8 消息中的跟踪时间参数。 |

## `summary.json.pagination`

| 顺序 | 字段 | 类型 | 含义 |
|---:|---|---|---|
| 1 | `ballComplete` | 布尔值 | 球 PRC 主动分页是否完成。 |
| 2 | `ballPageLimitReached` | 布尔值 | 球分页是否达到 64 页安全上限。 |
| 3 | `ballPagesReceived` | 整数 | 收到响应的球分页请求数。 |
| 4 | `ballPagesRequested` | 整数 | 发出的球分页请求数。 |
| 5 | `ballTimedOut` | 布尔值 | 球分页请求是否超时。 |
| 6 | `clubComplete` | 布尔值 | 杆头 PRC 主动分页是否完成。 |
| 7 | `clubPageLimitReached` | 布尔值 | 杆头分页是否达到 64 页安全上限。 |
| 8 | `clubPagesReceived` | 整数 | 收到响应的杆头分页请求数。 |
| 9 | `clubPagesRequested` | 整数 | 发出的杆头分页请求数。 |
| 10 | `clubTimedOut` | 布尔值 | 杆头分页请求是否超时。 |

## `summary.json.speedProfile`

| 顺序 | 字段 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `flags` | 位标志 | 速度曲线原始标志/版本字节。 |
| 2 | `numPost` | 整数 | 设备声明的撞击后样本数。 |
| 3 | `numPre` | 整数 | 设备声明的撞击前样本数。 |
| 4 | `scaleFactor` | 整数 | 线路速度整数比例；`speedsMps` 已完成除法换算。 |
| 5 | `speedsMps` | m/s 数组 | 杆头速度缓冲区；可能包含未检测样本或零填充。 |
| 6 | `timeIntervalSeconds` | 秒 | 相邻速度槽位的时间间隔。 |

## `summary.json.spin`

| 顺序 | 字段 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `amSpinRpm` | rpm | AM 算法通道的旋转估计。 |
| 2 | `antennaData` | `5 × 3` 数组 | 5 个天线组、每组 3 个旋转候选/bin。 |
| 2.1 | `antennaData[][].peak` | 缩放值 | 当前旋转候选的峰强度。 |
| 2.2 | `antennaData[][].snr` | 原始整数 | 当前旋转候选的信号质量量。 |
| 2.3 | `antennaData[][].spinRpm` | rpm | 当前天线组候选的旋转频率。 |
| 3 | `aodSpinRpm` | rpm | AOD 算法通道的旋转估计。 |
| 4 | `launchSpinRpm` | rpm | Mevo 采用的起飞旋转。 |
| 5 | `liftSpinRpm` | rpm | Lift 算法通道的旋转估计。 |
| 6 | `pllSpinRpm` | rpm | PLL 算法通道的旋转估计。 |
| 7 | `pmSpinConfidence` | 原始分数 | PM 旋转结果的置信度指标。 |
| 8 | `pmSpinFinalRpm` | rpm | PM 算法最终旋转结果。 |
| 9 | `pmSpinRawRpm` | rpm | PM 算法未完成最终修正的原始旋转候选。 |
| 10 | `pmSpinRpm` | rpm | PM 算法对外旋转结果。 |
| 11 | `spinAxisDeg` | 度 | 旋转轴角。 |
| 12 | `spinFlags` | 位标志 | 旋转结果原始状态位。 |
| 13 | `spinMethod` | 整数枚举 | Mevo 选择的旋转算法编号。 |
| 14 | `spinValidateExpectedRpm` | rpm | 旋转验证使用的期望值。 |
| 15 | `spinValidateHighRpm` | rpm | 旋转验证上界。 |
| 16 | `spinValidateLowRpm` | rpm | 旋转验证下界。 |
| 17 | `spinValidateScaling` | 原始数值 | 旋转验证缩放参数。 |
| 18 | `version` | 整数 | EF 消息版本/长度字节。 |
