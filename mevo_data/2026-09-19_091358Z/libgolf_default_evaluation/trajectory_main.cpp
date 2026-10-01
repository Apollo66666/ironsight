// Appended to the unchanged model-definition prefix of the existing optimized runner.
// Input/output SI coordinates: X forward, Y up, Z lateral (Mevo D4 convention).
int main(int argc, char **argv)
{
    try {
        if (argc != 3) throw std::runtime_error("usage: runner calibration.csv dt_s");
        const Calibration config = loadCalibration(argv[1]);
        const float dt = std::stof(argv[2]);
        if (!(dt > 0 && dt <= 0.01F)) throw std::runtime_error("invalid dt");
        std::string line;
        std::getline(std::cin, line);
        const auto header = splitCsvLine(line);
        std::map<std::string, std::size_t> columns;
        for (std::size_t i = 0; i < header.size(); ++i) columns[header[i]] = i;
        std::cout << "shot_id,model,t_s,X_m,Y_m,Z_m,VX_mps,VY_mps,VZ_mps\n";
        std::cout << std::setprecision(12);
        while (std::getline(std::cin, line)) {
            if (line.empty()) continue;
            const auto row = splitCsvLine(line);
            const auto value = [&](const std::string &key) {
                const auto v = std::stod(row.at(columns.at(key)));
                if (!std::isfinite(v)) throw std::runtime_error("nonfinite input");
                return static_cast<float>(v);
            };
            LaunchData launch{
                .ballSpeedMph = value("speed_mph"),
                .launchAngleDeg = value("elevation_deg"),
                .directionDeg = value("azimuth_deg"),
                .backspinRpm = value("backspin_rpm"),
                .sidespinRpm = value("libgolf_sidespin_rpm"),
                .startX = value("start_Z_m") / 0.3048F,
                .startY = value("start_X_m") / 0.3048F,
                .startZ = value("start_Y_m") / 0.3048F,
            };
            for (const std::string name : {"original", "optimized"}) {
                AtmosphericData atmosphere{}; // Actual library defaults; no overrides.
                GroundSurface ground{};
                BallProperties ball{};
                std::shared_ptr<AerodynamicModel> model;
                if (name == "original") model = std::make_shared<DefaultAerodynamicModel>();
                else model = std::make_shared<BivariateQuadraticModel>(config);
                ShotPhysicsContext physics(launch, atmosphere, ball);
                auto terrain = std::make_shared<FlatTerrain>(ground);
                AerialPhase aerial(physics, launch, atmosphere, terrain, model, ball);
                BallState state = BallState::fromLaunchParameters(
                    launch.ballSpeedMph * physics_constants::MPH_TO_FT_PER_S,
                    launch.launchAngleDeg, launch.directionDeg,
                    Vector3D{launch.startX, launch.startY, launch.startZ},
                    physics_constants::GRAVITY_FT_PER_S2, physics.getW());
                aerial.initialize(state);
                const auto emit = [&](double time, const Vector3D &p, const Vector3D &v) {
                    std::cout << row.at(columns.at("shot_id")) << ',' << name << ',' << time;
                    for (int axis : {1, 2, 0}) std::cout << ',' << p[axis] * kFeetToMeters;
                    for (int axis : {1, 2, 0}) std::cout << ',' << v[axis] * kFeetToMeters;
                    std::cout << '\n';
                };
                emit(state.currentTime, state.position, state.velocity);
                bool landed = false;
                for (int step = 0; step < static_cast<int>(120.0F / dt); ++step) {
                    const auto previous = state;
                    aerial.calculateStep(state, dt);
                    if (previous.position[2] > ground.height && state.position[2] <= ground.height) {
                        const double f = (previous.position[2] - ground.height) /
                            static_cast<double>(previous.position[2] - state.position[2]);
                        auto p = state.position;
                        auto v = state.velocity;
                        for (int axis = 0; axis < 3; ++axis) {
                            p[axis] = previous.position[axis] + f * (state.position[axis] - previous.position[axis]);
                            v[axis] = previous.velocity[axis] + f * (state.velocity[axis] - previous.velocity[axis]);
                        }
                        p[2] = ground.height;
                        emit(previous.currentTime + f * (state.currentTime - previous.currentTime), p, v);
                        landed = true;
                        break;
                    }
                    emit(state.currentTime, state.position, state.velocity);
                }
                if (!landed) throw std::runtime_error("no landing");
            }
        }
    } catch (const std::exception &e) {
        std::cerr << e.what() << '\n';
        return 1;
    }
}
