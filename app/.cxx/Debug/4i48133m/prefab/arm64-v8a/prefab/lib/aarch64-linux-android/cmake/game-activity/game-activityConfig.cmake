if(NOT TARGET game-activity::game-activity)
add_library(game-activity::game-activity STATIC IMPORTED)
set_target_properties(game-activity::game-activity PROPERTIES
    IMPORTED_LOCATION "/home/angsudo/.gradle/caches/transforms-4/88607b83a43a670f1e5898d353453c26/transformed/games-activity-4.4.0/prefab/modules/game-activity/libs/android.arm64-v8a/libgame-activity.a"
    INTERFACE_INCLUDE_DIRECTORIES "/home/angsudo/.gradle/caches/transforms-4/88607b83a43a670f1e5898d353453c26/transformed/games-activity-4.4.0/prefab/modules/game-activity/include"
    INTERFACE_LINK_LIBRARIES ""
)
endif()

if(NOT TARGET game-activity::game-activity_static)
add_library(game-activity::game-activity_static STATIC IMPORTED)
set_target_properties(game-activity::game-activity_static PROPERTIES
    IMPORTED_LOCATION "/home/angsudo/.gradle/caches/transforms-4/88607b83a43a670f1e5898d353453c26/transformed/games-activity-4.4.0/prefab/modules/game-activity_static/libs/android.arm64-v8a/libgame-activity_static.a"
    INTERFACE_INCLUDE_DIRECTORIES "/home/angsudo/.gradle/caches/transforms-4/88607b83a43a670f1e5898d353453c26/transformed/games-activity-4.4.0/prefab/modules/game-activity_static/include"
    INTERFACE_LINK_LIBRARIES ""
)
endif()

