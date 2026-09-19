# 相机数据字段表

## `camera_ball_raw.csv`

| 顺序 | 字段 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `shot_id` | 整数 | 所属 Session 内的击球编号。 |
| 2 | `guid` | 字符串 | 一次击球在消息流中的唯一关联标识。 |
| 3 | `track_id` | 整数 | 目标类型；此文件固定为 0，表示球。 |
| 4 | `point_index` | 整数 | 当前 Track 平行数组中的导出下标。 |
| 5 | `frame_number` | 整数 | 检测到球的相机帧编号。 |
| 6 | `timestamp` | Unix 秒 | 对应帧的 Epoch 时间戳。 |
| 7 | `u_px` | 像素 | 球中心的水平像素坐标。 |
| 8 | `v_px` | 像素 | 球中心的竖直像素坐标。 |
| 9 | `radius_px` | 像素 | 球检测区域的等效半径。 |
| 10 | `circularity_factor` | 形状指标 | 球检测轮廓的圆形程度/形状质量量。 |
| 11 | `shutter_time_ms` | ms | 当前帧的曝光时间。 |

## `camera_club_raw.csv`

| 顺序 | 字段 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `shot_id` | 整数 | 所属 Session 内的击球编号。 |
| 2 | `guid` | 字符串 | 一次击球在消息流中的唯一关联标识。 |
| 3 | `track_id` | 整数 | 目标类型；此文件固定为 1，表示杆头。 |
| 4 | `point_index` | 整数 | 当前 Track 平行数组中的导出下标。 |
| 5 | `frame_number` | 整数 | 检测到杆头的相机帧编号。 |
| 6 | `timestamp` | Unix 秒 | 对应帧的 Epoch 时间戳。 |
| 7 | `u_px` | 像素 | 杆头检测中心的水平像素坐标。 |
| 8 | `v_px` | 像素 | 杆头检测中心的竖直像素坐标。 |
| 9 | `radius_px` | 像素 | 杆头检测区域的等效半径。 |
| 10 | `circularity_factor` | 形状指标 | 杆头检测的形状指标；数值尺度不能与球 Track 直接比较。 |
| 11 | `shutter_time_ms` | ms | 当前帧的曝光时间。 |

## `camera_reference_raw.csv`

| 顺序 | 字段 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `shot_id` | 整数 | 所属 Session 内的击球编号。 |
| 2 | `guid` | 字符串 | 一次击球在消息流中的唯一关联标识。 |
| 3 | `track_id` | 整数 | 大于等于 2 的参考/辅助 Track 编号。 |
| 4 | `point_index` | 整数 | 当前 Track 平行数组中的导出下标。 |
| 5 | `frame_number` | 整数 | 参考目标所在的相机帧编号。 |
| 6 | `timestamp` | Unix 秒 | 对应帧的 Epoch 时间戳。 |
| 7 | `u_px` | 像素 | 参考目标的水平像素坐标。 |
| 8 | `v_px` | 像素 | 参考目标的竖直像素坐标。 |
| 9 | `radius_px` | 像素 | 参考目标检测区域的等效半径。 |
| 10 | `circularity_factor` | 形状指标 | 参考目标的形状/质量指标。 |
| 11 | `shutter_time_ms` | ms | 当前帧的曝光时间。 |

## `gvp_result.json`

| 顺序 | 字段路径 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `guid` | 字符串 | 当前相机结果对应的击球 GUID。 |
| 2 | `cameraCalibration` | 对象 | GVP 返回的相机内参、外参和畸变参数集合。 |
| 2.1 | `cameraCalibration.cx` | 像素 | 相机成像主点的水平坐标。 |
| 2.2 | `cameraCalibration.cy` | 像素 | 相机成像主点的竖直坐标。 |
| 2.3 | `cameraCalibration.fx` | 像素焦距 | 相机水平方向焦距。 |
| 2.4 | `cameraCalibration.fy` | 像素焦距 | 相机竖直方向焦距。 |
| 2.5 | `cameraCalibration.width` | 像素 | 相机标定对应的图像宽度。 |
| 2.6 | `cameraCalibration.height` | 像素 | 相机标定对应的图像高度。 |
| 2.7 | `cameraCalibration.position` | 三元素数组 | 相机在外部坐标系中的三维位置。 |
| 2.8 | `cameraCalibration.rotation` | 三元素数组 | 相机相对于外部坐标系的三维旋转。 |
| 2.9 | `cameraCalibration.distCoeffs` | 八元素数组 | 镜头畸变模型系数。 |
| 3 | `tracks` | 对象数组 | GVP 输出的全部目标 Track。 |
| 3.1 | `tracks[].trackId` | 整数 | 目标编号：0 为球、1 为杆头、2～4 通常为参考标记。 |
| 3.2 | `tracks[].frameNumber` | 整数数组 | 每个检测点对应的相机帧编号。 |
| 3.3 | `tracks[].timestamp` | Unix 秒数组 | 每个检测点对应的帧时间戳。 |
| 3.4 | `tracks[].u` | 像素数组 | 每个检测点的水平像素坐标。 |
| 3.5 | `tracks[].v` | 像素数组 | 每个检测点的竖直像素坐标。 |
| 3.6 | `tracks[].radius` | 像素数组 | 每个检测区域的等效半径。 |
| 3.7 | `tracks[].circularityFactor` | 数值数组 | 每个检测点的形状指标。 |
| 3.8 | `tracks[].shutterTime_ms` | ms 数组 | 每个检测帧的曝光时间。 |

## `gvp_result_raw.json`

| 顺序 | 字段路径 | 类型/单位 | 含义 |
|---:|---|---|---|
| 1 | `cameraCalibration` | 对象 | 原始消息中的相机标定对象。 |
| 1.1 | `cameraCalibration.cx` | 像素 | 相机主点水平坐标。 |
| 1.2 | `cameraCalibration.cy` | 像素 | 相机主点竖直坐标。 |
| 1.3 | `cameraCalibration.distCoeffs` | 八元素数组 | 镜头畸变系数。 |
| 1.4 | `cameraCalibration.fx` | 像素焦距 | 水平方向焦距。 |
| 1.5 | `cameraCalibration.fy` | 像素焦距 | 竖直方向焦距。 |
| 1.6 | `cameraCalibration.height` | 像素 | 标定图像高度。 |
| 1.7 | `cameraCalibration.position` | 三元素数组 | 相机位置外参。 |
| 1.8 | `cameraCalibration.rotation` | 三元素数组 | 相机旋转外参。 |
| 1.9 | `cameraCalibration.width` | 像素 | 标定图像宽度。 |
| 2 | `guid` | 字符串 | 当前相机结果对应的击球 GUID。 |
| 3 | `tracks` | 对象数组 | 原始 RESULT 中的全部目标轨迹和检测候选。 |
| 3.1 | `tracks[].circularityFactor` | 数值数组 | 各检测点形状指标。 |
| 3.2 | `tracks[].frameNumber` | 整数数组 | 各检测点帧编号。 |
| 3.3 | `tracks[].radius` | 像素数组 | 各检测区域等效半径。 |
| 3.4 | `tracks[].shutterTime_ms` | ms 数组 | 各检测帧曝光时间。 |
| 3.5 | `tracks[].timestamp` | Unix 秒数组 | 各检测点帧时间戳。 |
| 3.6 | `tracks[].trackId` | 整数 | 目标编号。 |
| 3.7 | `tracks[].u` | 像素数组 | 各检测点水平像素坐标。 |
| 3.8 | `tracks[].v` | 像素数组 | 各检测点竖直像素坐标。 |
| 4 | `type` | 字符串 | GVP 消息类型。 |
| 5 | `version` | 整数 | GVP RESULT 消息结构版本。 |
