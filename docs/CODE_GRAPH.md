# vadadee-berry Code Graph

_Generated solely with the `tree-sitter` CLI (`tags` + `query`, tree-sitter-rust grammar). Callers are attributed to the nearest preceding `fn` in the same file._

## Files

### `src/animation.rs`

- **class** `UiAnimation` (L19)
- **method** `default` (L79)
- **method** `new` (L85)
- **method** `tick` (L219)
- **method** `begin_timeline_slide` (L253)
- **method** `advance_timeline_slide` (L260)
- **method** `apply_timeline_pose` (L276)
- **method** `settle_timeline_pose` (L286)
- **method** `begin_video_editor_slide` (L294)
- **method** `advance_video_editor_slide` (L301)
- **method** `apply_video_editor_pose` (L317)
- **method** `settle_video_editor_pose` (L327)
- **method** `begin_left_dock_slide` (L335)
- **method** `advance_left_dock_slide` (L342)
- **method** `sync_left_dock` (L361)
- **method** `left_dock_open_t` (L378)
- **method** `left_dock_opacity` (L382)
- **method** `begin_action_bar_slide` (L386)
- **method** `advance_action_bar_slide` (L393)
- **method** `apply_action_bar_pose` (L409)
- **method** `settle_action_bar_pose` (L420)
- **method** `sync` (L428)
- **method** `sync_on_path` (L534)
- **method** `on_path_offer_pop` (L551)
- **method** `on_path_container_expand` (L558)
- **method** `on_path_container_alpha` (L568)
- **method** `on_tab_change` (L573)
- **method** `on_tab_change_secondary` (L580)
- **method** `is_active` (L586)
- **method** `needs_repaint` (L592)
- **method** `action_bar_slide_running` (L618)
- **method** `action_bar_open_t` (L622)
- **method** `action_bar_opacity` (L627)
- **method** `range_inclusive` (L631)
- **method** `menubar_alpha` (L636)
- **method** `toolbar_alpha` (L640)
- **method** `status_alpha` (L644)
- **method** `canvas_alpha` (L648)
- **method** `tab_content_alpha` (L652)
- **method** `tab_content_offset` (L656)
- **method** `tool_highlight` (L660)
- **method** `tab_label_alpha` (L664)
- **method** `status_slide_out` (L672)
- **method** `status_slide_in` (L676)
- **method** `status_tool_slide_out` (L680)
- **method** `status_tool_slide_in` (L684)
- **method** `status_tool_seg_width` (L688)
- **method** `status_message_seg_width` (L700)
- **method** `coords_slide_out` (L717)
- **method** `coords_slide_in` (L721)
- **method** `coords_seg_width` (L725)
- **method** `play_intro` (L734)
- **method** `replay_intro` (L750)
- **method** `seed_status_board` (L754)
- **function** `left_dock_panel_rect` (L791)
- **function** `action_bar_overlay_rect` (L806)
- **function** `lerp_color` (L819)
- **interface** `KramaFrameExt` (L829)
- **method** `set_progress_max` (L834)
  - uses: `egui::{Color32,Context,Rect}`
  - uses: `kramaframe::keylist::TRES16Bits`
  - uses: `kramaframe::prelude::{KeyFrameFunction,KeyList}`
  - uses: `kramaframe::{BTclasslist,BTframelist,KramaFrame}`
  - uses: `crate::tools::ToolKind`
  - uses: `crate::ui::ActionTab`
  - uses: `crate::theme`
  - uses: `crate::theme`

### `src/app.rs`

- **class** `GradientFlowTarget` (L31)
- **class** `GraphGpuFxEntry` (L38)
- **class** `GradientFlowDrag` (L50)
- **class** `ImagePastePlacement` (L59)
- **class** `PasteTask` (L67)
- **class** `PasteProgress` (L83)
- **class** `AnimAppliedState` (L92)
- **class** `SnapGuide` (L106)
- **class** `VadadeeBerryApp` (L112)
- **class** `FloodFillAnim` (L413)
- **class** `McpPreviewState` (L434)
- **class** `McpPreviewUpdate` (L446)
- **class** `TextPanAnim` (L455)
- **class** `VideoFrameCache` (L463)
- **class** `VideoCommand` (L469)
- **class** `VideoLayerState` (L482)
- **class** `VideoExportState` (L508)
- **method** `fmt` (L558)
- **method** `default` (L583)
- **function** `collab_wire_hash` (L666)
- **method** `new` (L674)
- **method** `window_title` (L984)
- **method** `sync_window_title` (L996)
- **method** `ensure_audio_output` (L1000)
- **method** `reset_audio_output` (L1032)
- **method** `save_project_to_path` (L1043)
- **method** `doc_at_pointer_hover` (L1050)
- **method** `ensure_cursor_doc_for_collab_bubble` (L1061)
- **method** `update_cursor_doc_from_pointer` (L1077)
- **method** `canvas_has_active_focus` (L1104)
- **method** `focus_viewport_on_peer` (L1109)
- **method** `apply_collab_remote_ui` (L1119)
- **method** `tick_live_collaboration_poll` (L1136)
- **method** `tick_live_collaboration_after_canvas` (L1200)
- **method** `new_document` (L1279)
- **method** `request_open_svg` (L1294)
- **method** `request_open_project` (L1298)
- **method** `request_import_image` (L1302)
- **method** `request_save_project` (L1346)
- **method** `request_export_svg` (L1349)
- **method** `request_export_image` (L1353)
- **method** `apply_media_duration_from_path` (L1357)
- **method** `apply_probed_media_duration` (L1382)
- **method** `sync_stale_media_layer_durations` (L1396)
- **method** `refresh_all_media_layer_durations` (L1417)
- **method** `do_undo` (L1452)
- **method** `do_redo` (L1465)
- **method** `after_history_step` (L1480)
- **method** `apply_document_changes` (L1492)
- **method** `restore_selection_after_history` (L1503)
- **method** `clear_transient_tool_state` (L1515)
- **method** `abort_raster_stroke_uncommitted` (L1531)
- **method** `dismiss_on_page_text_edit_without_history` (L1553)
- **method** `set_selection` (L1561)
- **method** `try_delete_focused_gradient_stop` (L1567)
- **method** `sync_inspector_from_selection` (L1602)
- **method** `inspector_opacity` (L1694)
- **method** `apply_fill_to_selection` (L1702)
- **method** `reverse_path` (L1725)
- **method** `set_all_path_anchors_smooth` (L1744)
- **method** `simplify_path` (L1767)
- **method** `set_path_closed` (L1786)
- **method** `set_circle_geometry` (L1800)
- **method** `set_polygon_geometry` (L1804)
- **method** `build_ui_fill` (L1839)
- **method** `build_ui_stroke` (L1854)
- **method** `build_brush_fill` (L1882)
- **method** `apply_stroke_to_selection` (L1897)
- **method** `apply_path_markers_to_selection` (L1916)
- **method** `apply_stroke_width_to_selection` (L1932)
- **method** `apply_no_stroke_to_selection` (L1946)
- **method** `set_selection_opacity` (L1960)
- **method** `rename_node` (L1974)
- **method** `set_rect_geometry` (L1986)
- **method** `set_ellipse_geometry` (L2020)
- **method** `set_line_geometry` (L2051)
- **method** `set_path_origin` (L2078)
- **method** `set_plotter_geometry` (L2105)
- **method** `set_plotter_expr_live` (L2173)
- **method** `begin_plotter_expr_edit` (L2185)
- **method** `commit_plotter_expr_edit` (L2195)
- **method** `cancel_plotter_expr_edit` (L2216)
- **method** `set_arc_geometry` (L2228)
- **method** `set_flowchart_node_label` (L2269)
- **method** `set_flowchart_path_props` (L2310)
- **method** `set_document_title` (L2334)
- **method** `set_page_size` (L2344)
- **method** `get_content_max_animation_frame` (L2357)
- **method** `get_max_animation_frame` (L2431)
- **method** `prune_orphan_animation_tracks` (L2438)
- **method** `animation_content_duration_secs` (L2442)
- **method** `cache_current_project_for_open` (L2447)
- **method** `sync_anim_transform_from_node` (L2460)
- **method** `write_anim_keyframe_at_edit` (L2481)
- **method** `apply_animation_for_frame` (L2505)
- **method** `apply_node_editor_param_animation` (L2710)
- **method** `get_node_geom_floats` (L2798)
- **method** `set_node_geom_floats` (L2839)
- **method** `convert_rect_to_path` (L2926)
- **method** `toggle_keyframing_mode` (L2998)
- **method** `add_layer` (L3030)
- **method** `add_empty_av_layer` (L3041)
- **method** `add_empty_av_layer_with_role` (L3045)
- **method** `add_shading_layer` (L3056)
- **method** `add_flowchart_layer` (L3060)
- **method** `add_node_editor_layer` (L3071)
- **method** `add_screen_record_layer` (L3094)
- **method** `start_screen_record` (L3112)
- **method** `start_screen_record` (L3174)
- **method** `stop_screen_record` (L3180)
- **method** `stop_screen_record` (L3217)
- **method** `is_screen_recording` (L3222)
- **method** `add_graph_node_to_active` (L3234)
- **method** `rebalance_active_flowchart_layer_if_any` (L3253)
- **method** `add_shading_layer_with_preset` (L3266)
- **method** `add_shading_layer_with_wgsl` (L3277)
- **method** `load_shading_wgsl_from_path` (L3303)
- **method** `set_shading_wgsl` (L3321)
- **method** `add_shading_layer_with_passes` (L3366)
- **method** `av_role_for_media_path` (L3388)
- **method** `add_av_layer` (L3401)
- **method** `push_media_clip` (L3415)
- **method** `push_selection_as_av_image_clip` (L3544)
- **method** `rasterize_nodes_to_png` (L3590)
- **method** `object_link_content_sig` (L3645)
- **method** `refresh_object_linked_av_clips` (L3687)
- **method** `delete_av_clip` (L3862)
- **method** `add_video_layer` (L3896)
- **method** `add_audio_layer` (L3899)
- **method** `set_active_layer` (L3904)
- **method** `set_layer_visible` (L3918)
- **method** `set_layer_locked` (L3930)
- **method** `rename_layer` (L3942)
- **method** `live_action_status` (L3954)
- **method** `is_ephemeral_status_event` (L4007)
- **method** `update_window_focus_status` (L4046)
- **method** `derive_action_status` (L4066)
- **method** `selection_bounds` (L4111)
- **method** `selection_bounds_for_raster` (L4117)
- **method** `export_raster_scale` (L4122)
- **method** `resize_to_selection` (L4126)
- **method** `copy_selection_as_png` (L4160)
- **method** `request_video_export` (L4206)
- **method** `begin_video_export` (L4224)
- **method** `cancel_video_export` (L4314)
- **method** `finish_video_export_ui` (L4322)
- **method** `poll_video_export` (L4338)
- **method** `copy_selection` (L4466)
- **method** `cut_selection` (L4486)
- **method** `paste_anchor_doc` (L4513)
- **method** `view_center_doc` (L4522)
- **method** `doc_point_in_view` (L4533)
- **method** `nearby_nudge_doc` (L4543)
- **method** `image_paste_doc_center` (L4564)
- **method** `object_paste_offset` (L4568)
- **method** `begin_system_image_paste` (L4599)
- **method** `begin_object_paste` (L4611)
- **method** `finish_paste` (L4625)
- **method** `advance_paste_operation` (L4630)
- **method** `is_pasting` (L4771)
- **method** `system_clipboard_has_image` (L4776)
- **method** `system_clipboard_has_image` (L4784)
- **method** `paste_clipboard` (L4789)
- **method** `group_selection` (L4821)
- **method** `ungroup_selection` (L4880)
- **method** `duplicate_selection` (L4928)
- **method** `delete_layer` (L4960)
- **method** `nudge_layer_order` (L4977)
- **method** `layer_index_for_node_selection` (L4995)
- **method** `nudge_nodes_within_layer` (L5009)
- **method** `selected_layer_kind` (L5046)
- **method** `nudge_z_order` (L5050)
- **method** `flip_selection` (L5079)
- **method** `layer_editable` (L5162)
- **method** `process_file_dialogs` (L5170)
- **method** `object_clipboard_blocked` (L5336)
- **method** `handle_text_paste_fallback` (L5343)
- **method** `handle_object_clipboard_shortcuts` (L5382)
- **method** `handle_paste_hotkey_fallback` (L5579)
- **method** `keyboard_shortcuts` (L5605)
- **method** `cancel_tool_to_select` (L5914)
- **method** `cancel_weight_flow_stroke` (L5957)
- **method** `delete_keyframe` (L5968)
- **method** `get_node_geom_track_name` (L5992)
- **method** `delete_selection_public` (L6106)
- **method** `delete_selection` (L6110)
- **method** `insert_node` (L6203)
- **method** `insert_nodes_batch` (L6215)
- **method** `mcp_bulk_active` (L6228)
- **method** `apply_nodes_live` (L6239)
- **method** `rebuild_spatial_index` (L6246)
- **method** `draw_order_cached` (L6259)
- **method** `is_bulk_selection` (L6267)
- **method** `sync_inspector_if_needed` (L6271)
- **method** `setup_bulk_drag_if_needed` (L6282)
- **method** `apply_bulk_move_preview` (L6304)
- **method** `revert_bulk_move_preview` (L6325)
- **method** `commit_bulk_drag` (L6344)
- **method** `split_active_av_clip_at_playhead` (L6383)
- **method** `create_music_clip_at_playhead` (L6425)
- **method** `create_daw_clip_at_playhead` (L6430)
- **method** `select_from_hit_picker` (L6469)
- **method** `pick_node_at` (L6479)
- **method** `pick_node_at_opts` (L6483)
- **method** `pick_clip_mask_at` (L6495)
- **method** `clip_pair_for` (L6525)
- **method** `pick_node_at_with_bbox_fallback` (L6534)
- **method** `pick_node_at_with_bbox_fallback_opts` (L6542)
- **method** `pick_all_nodes_at` (L6554)
- **method** `ensure_image_texture` (L6593)
- **method** `invalidate_image_textures` (L6613)
- **method** `sync_image_texture_from_rgba` (L6619)
- **method** `raster_paint_preview_color` (L6645)
- **method** `raster_paint_rgba` (L6650)
- **method** `paint_buffer_dims` (L6674)
- **method** `raster_new_paint_layer` (L6690)
- **method** `raster_new_paint_layer_from_selection` (L6727)
- **method** `raster_paint_target_info` (L6737)
- **method** `raster_float_selection` (L6752)
- **method** `raster_refresh_float_preview` (L6970)
- **method** `composite_float_into` (L6989)
- **method** `raster_apply_float` (L7044)
- **method** `raster_cancel_float` (L7085)
- **method** `raster_nudge_float` (L7110)
- **method** `raster_scale_float` (L7120)
- **method** `raster_rotate_float` (L7129)
- **method** `raster_float_pointer` (L7139)
- **method** `ensure_raster_paint_target` (L7210)
- **method** `tool_raster_paint` (L7244)
- **method** `raster_stamp_at` (L7325)
- **method** `draw_raster_stroke_overlay` (L7569)
- **method** `flush_raster_texture` (L7672)
- **method** `finish_raster_stroke` (L7691)
- **method** `raster_sym_origin_px` (L7766)
- **method** `tool_bucket_fill` (L7789)
- **method** `tick_flood_fill_anim` (L7917)
- **method** `raster_paint_clip_px` (L7995)
- **method** `raster_poly_masks_px` (L8042)
- **method** `raster_poly_mask_px` (L8080)
- **method** `raster_pixel_in_sticky_mask_px` (L8097)
- **method** `doc_aabb_to_image_clip` (L8152)
- **method** `raster_selection_clip_px` (L8210)
- **method** `raster_set_sticky_mask_from_selection` (L8247)
- **method** `raster_clear_sticky_mask` (L8258)
- **method** `raster_invert_mask` (L8274)
- **method** `raster_grow_mask` (L8418)
- **method** `raster_shrink_mask` (L8435)
- **method** `raster_select_all_image` (L8454)
- **method** `raster_apply_rect_mask` (L8486)
- **method** `raster_apply_poly_mask` (L8515)
- **method** `raster_select_color_region` (L8549)
- **method** `tool_raster_select` (L8691)
- **method** `selection_is_single_image` (L8752)
- **method** `raster_reset_sym_origin` (L8756)
- **method** `handle_paint_mask_draw` (L8763)
- **method** `draw_dashed_polyline` (L8846)
- **method** `draw_paint_mask_overlay` (L8905)
- **method** `draw_pixel_mask_selection_overlay` (L8989)
- **method** `draw_raster_select_overlay` (L9179)
- **method** `raster_clear_layer` (L9222)
- **method** `handle_symmetry_origin_gizmo` (L9280)
- **method** `draw_circular_symmetry_guides` (L9330)
- **method** `raster_pick_color_at` (L9391)
- **method** `ensure_graph_path_texture` (L9417)
- **method** `ensure_graph_path_texture_at` (L9421)
- **method** `ensure_graph_fx_texture_public` (L9488)
- **method** `graph_path_texture_id` (L9497)
- **method** `graph_path_texture_size` (L9502)
- **method** `ensure_graph_bake_texture` (L9507)
- **method** `image_texture_id` (L9530)
- **method** `ensure_graph_fx_texture` (L9536)
- **method** `invalidate_graph_gpu_live` (L9715)
- **method** `invalidate_graph_gpu_path_prefix` (L9725)
- **method** `graph_fx_paint_tex` (L9744)
- **method** `run_video_decode_thread` (L9795)
- **method** `stop_all_video_streams` (L9854)
- **method** `tick_video_layers` (L9869)
- **method** `cleanup_unused_audio_caches` (L10108)
- **method** `insert_image` (L10247)
- **method** `finish_pen_path` (L10253)
- **method** `sync_pen_continue_from_selection` (L10289)
- **method** `canvas_ui` (L10322)
- **method** `handle_canvas_input` (L11769)
- **method** `commit_drag_edits` (L12090)
- **method** `sync_flowchart_paths_if_active_layer` (L12134)
- **method** `update_clip_mask_textures` (L12172)
- **method** `hidden_canvas_sources` (L12225)
- **method** `update_layer_raster_cache` (L12277)
- **method** `sync_audio_playback` (L12358)
- **method** `set_path_handle_mode` (L12683)
- **method** `set_path_anchor_smooth` (L12701)
- **method** `make_corner_curve` (L12732)
- **method** `smooth_selected_path_points` (L12777)
- **method** `remove_selected_path_points` (L12809)
- **method** `selection_path_and_objects` (L12844)
- **method** `selection_path_and_object` (L12848)
- **method** `sync_on_path_ui_from_selection` (L12852)
- **method** `sync_tiling_ui_from_selection` (L12888)
- **method** `sync_circular_ui_from_selection` (L12911)
- **method** `get_tiling_gizmo_points` (L12929)
- **method** `get_circular_gizmo_points` (L12942)
- **method** `hit_circular_gizmo` (L12955)
- **method** `dist_point_to_segment_screen` (L12998)
- **method** `sync_circular_ui_from_effect_id` (L13009)
- **method** `translate_circular_effect_for_source` (L13026)
- **method** `build_on_path_effect` (L13045)
- **method** `object_on_path_panel_context` (L13074)
- **method** `selection_has_object_on_path_effect` (L13078)
- **method** `is_tiling_circular_source` (L13083)
- **method** `selection_tiling_circular_sources` (L13087)
- **method** `selection_has_tiling_effect` (L13091)
- **method** `selection_has_circular_effect` (L13095)
- **method** `convert_selection_to_path` (L13100)
- **method** `snap_gizmo_point` (L13168)
- **method** `node_has_tiling_or_circular` (L13245)
- **method** `node_uses_extended_bounds` (L13255)
- **method** `hit_test_node_for_pick` (L13260)
- **method** `precise_hit_for_pick` (L13288)
- **method** `expand_drag_ids_for_path_effects` (L13315)
- **method** `apply_object_on_path_effect` (L13328)
- **method** `backfill_path_effect_forms_if_needed` (L13399)
- **method** `update_object_on_path_effects_live` (L13442)
- **method** `update_tiling_effects_live` (L13485)
- **method** `update_circular_effects_live` (L13505)
- **method** `remove_object_on_path_effect` (L13520)
- **method** `remove_one_object_on_path_effect` (L13529)
- **method** `bake_object_on_path_copies` (L13594)
- **method** `apply_tiling_magic` (L13665)
- **method** `apply_circular_clone_magic` (L13724)
- **method** `remove_tiling_effect` (L13772)
- **method** `remove_circular_effect` (L13791)
- **method** `selection_boolean_pair` (L13811)
- **method** `selection_booleanable_shapes` (L13819)
- **method** `selection_boolean_mode` (L13835)
- **method** `selection_has_boolean_effect` (L13870)
- **method** `selection_has_clip_mask` (L13880)
- **method** `find_boolean_effect_for_selection` (L13890)
- **method** `apply_boolean_effect` (L13905)
- **method** `apply_multi_boolean_fold` (L14004)
- **method** `reverse_boolean_operands` (L14066)
- **method** `set_boolean_op_live` (L14092)
- **method** `refresh_boolean_effects_live` (L14113)
- **method** `bake_boolean_effect` (L14160)
- **method** `remove_boolean_effect` (L14174)
- **method** `apply_clip_mask` (L14194)
- **method** `remove_clip_mask` (L14232)
- **method** `bake_clip_mask_to_raster` (L14262)
- **method** `swap_clip_mask_source` (L14408)
- **method** `bake_tiling` (L14450)
- **method** `bake_instance_to_path_node` (L14493)
- **method** `circular_bake_instances` (L14504)
- **method** `bake_circular` (L14523)
- **method** `bake_circular_as_path` (L14605)
- **method** `split_circular` (L14690)
- **method** `close_open_paths_in_selection` (L14751)
- **method** `open_closed_paths_in_selection` (L14771)
- **method** `begin_on_page_text_edit` (L14791)
- **method** `ease_text_pan` (L14815)
- **method** `start_text_pan_anim` (L14820)
- **method** `begin_text_focus_pan` (L14833)
- **method** `restore_text_focus_pan` (L14868)
- **method** `update_text_pan_animation` (L14874)
- **method** `patch_on_page_text_live` (L14891)
- **method** `finish_on_page_text_edit` (L14912)
- **method** `expand_ids_for_delete` (L14975)
- **method** `prune_node_editor_object_links` (L14995)
- **method** `eval_node_editor_graphs` (L15009)
- **method** `delete_nodes` (L15023)
- **method** `delete_on_page_text_node` (L15092)
- **method** `apply_fill_style_to_active` (L15143)
- **method** `sample_image_color` (L15250)
- **method** `color_at_doc_pos` (L15284)
- **method** `tool_eyedropper_holding` (L15323)
- **method** `tool_eyedropper` (L15366)
- **method** `set_text_style` (L15415)
- **method** `weight_flow_target_path` (L15442)
- **method** `is_live_geometry_editing` (L15452)
- **method** `sync_anim_geom_from_node` (L15461)
- **method** `record_geom_keyframes_for_node` (L15487)
- **method** `tool_weight_flow` (L15544)
- **method** `tool_select` (L15675)
- **method** `get_node_snap_points` (L16639)
- **method** `get_canvas_snap_points` (L16727)
- **method** `try_equal_spacing_snap` (L16751)
- **method** `snap_cursor` (L16853)
- **method** `apply_snapping` (L16921)
- **method** `styled_shape_node` (L17195)
- **method** `tool_drag_shape` (L17201)
- **method** `pen_push_anchor` (L17423)
- **method** `tool_pen` (L17466)
- **method** `handle_gradient_flow_input` (L17545)
- **method** `canvas_wheel_zoom` (L17763)
- **method** `tool_text` (L17791)
- **method** `tool_brush` (L17814)
- **method** `pixel_erase_begin` (L18076)
- **method** `pixel_erase_at` (L18097)
- **method** `pixel_erase_commit` (L18160)
- **method** `hit_path_segment` (L18233)
- **method** `hit_node_edit` (L18269)
- **method** `tool_node` (L18319)
- **method** `mcp_kind_label` (L18669)
- **method** `mcp_paint_hex` (L18687)
- **method** `mcp_truncate_str` (L18698)
- **method** `mcp_list_all_objects_json` (L18717)
- **method** `mcp_capture_canvas_raster` (L18752)
- **method** `mcp_list_objects_json` (L18838)
- **method** `mcp_get_object_json` (L18879)
- **method** `mcp_ensure_editable` (L18903)
- **method** `mcp_finish_node` (L18910)
- **method** `mcp_resolve_ne_layer_idx` (L18923)
- **method** `mcp_with_node_graph_mut` (L18961)
- **method** `mcp_node_editor_tool` (L18990)
- **method** `mcp_brush` (L19363)
- **method** `mcp_drawing_tool` (L19731)
- **method** `mcp_patch_nodes` (L20245)
- **method** `mcp_set_objects_style_from_args` (L20267)
- **method** `mcp_resolve_start_const` (L20291)
- **method** `mcp_add_stack_animation` (L20317)
- **method** `mcp_edit_stack_animation` (L20422)
- **method** `mcp_remove_stack_animation` (L20505)
- **method** `mcp_list_stack_animations` (L20531)
- **method** `mcp_list_animatable_properties` (L20578)
- **method** `mcp_list_animation_tracks` (L20622)
- **method** `mcp_get_object_properties` (L20687)
- **method** `mcp_node_kind_name` (L20749)
- **method** `mcp_set_selection` (L20766)
- **method** `mcp_duplicate_object` (L20785)
- **method** `mcp_reorder_object` (L20812)
- **method** `mcp_list_layers` (L20844)
- **method** `mcp_set_keyframe` (L20870)
- **method** `mcp_remove_keyframe` (L20906)
- **method** `mcp_get_keyframes` (L20912)
- **method** `mcp_set_keyframe_interpolation` (L20973)
- **method** `mcp_clear_animation_track` (L21015)
- **method** `mcp_set_keyframes` (L21037)
- **method** `mcp_patch_node` (L21075)
- **method** `mcp_apply_geometry_patch` (L21116)
- **method** `mcp_update_object` (L21290)
- **method** `mcp_delete_object` (L21294)
- **method** `process_pending_mcp_bulk_rects` (L21304)
- **method** `poll_mcp_bridge` (L21349)
- **method** `handle_mcp_request` (L21461)
- **method** `on_exit` (L21694)
- **method** `logic` (L21712)
- **method** `ui` (L22238)
- **function** `strip_pixel_rects_from_bez` (L22244)
- **function** `mcp_brush_xy` (L22334)
- **function** `mcp_brush_cell_color` (L22347)
- **function** `pixel_stamps_to_path` (L22396)
- **function** `densify_brush_centerline` (L22432)
- **function** `generate_brush_outline` (L22494)
- **module** `tests` (L22607)
- **method** `test_bezier_interpolation` (L22611)
- **method** `test_pure_motion_geometry_equivalence` (L22627)
- **method** `new_for_test` (L22669)
- **method** `test_gradient_color_animation` (L22927)
- **function** `is_video_container_ext` (L22984)
- **method** `ne_audio_extract_busy` (L22990)
- **method** `warm_ne_video_audio_extract` (L23000)
- **function** `status_one_line` (L23070)
- **function** `cached_wav_path_for_video` (L23086)
- **function** `sidecar_wav_path_for_video` (L23095)
- **function** `find_playable_extracted_wav` (L23105)
- **function** `spawn_video_audio_extract` (L23119)
- **function** `dirs_next_audio_cache_dir` (L23199)
- **function** `dirs_vadadee_cache_root` (L23215)
- **function** `dirs_next_screen_cache_dir` (L23230)
- **function** `purge_vadadee_disk_caches` (L23236)
- **class** `CachePurgeOpts` (L23286)
- **method** `on_startup` (L23299)
- **method** `on_exit` (L23309)
- **function** `purge_dir_files` (L23319)
- **function** `resolve_audio_path_for_rodio` (L23375)
- **function** `apply_color_controls` (L23437)
- **function** `adjust_frame_color` (L23480)
- **function** `paint_rotated_image` (L23500)
- **function** `paint_rotated_image_mirrored` (L23510)
- **function** `paint_rotated_image_mirrored_tint` (L23525)
- **function** `paint_rotated_image_mirrored_tint_uv` (L23549)
  - uses: `eframe::egui`
  - uses: `egui::{Context,Event,Key,Pos2,Sense,Ui}`
  - uses: `kurbo::Shape`
  - uses: `crate::animation::UiAnimation`
  - uses: `crate::canvas::Viewport`
  - uses: `crate::fonts::FontRegistry`
  - uses: `crate::commands::{CommandContext,CommandDispatcher,DocumentChanged,EditorCommand}`
  - uses: `crate::history::{snapshot_document,snapshot_project,History,ProjectEdit}`
  - uses: `crate::io`
  - uses: `crate::render`
  - uses: `crate::theme`
  - uses: `crate::tools::{self,DragNewShape,MarqueeSelect,SelectDrag,ToolKind,ToolState}`
  - uses: `crate::audio_extract::AudioExtractStatus`
  - uses: `crate::document::BooleanPairMode`
  - uses: `crate::ui`
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `crate::document::{InterpolationMode,Keyframe,KeyframeTrack,NodeAnimation,AnimationTimeline}`
  - uses: `crate::document::AnimGraphStackDrag`
  - uses: `crate::export_types::{ExportFxQuality,ExportPowerLevel,VideoBackend,VideoFormat}`
  - uses: `std::hash::{Hash,Hasher}`
  - uses: `crate::document::normalize_stops`
  - uses: `crate::gradient_ui::GradientEditorFocus`
  - uses: `crate::document::ShadingPass`
  - uses: `crate::document::ShadingPass`
  - uses: `crate::document::AvClip`
  - uses: `crate::document::{AvClip,AvRole,LayerKind}`
  - uses: `crate::document::{AvRole,LayerKind}`
  - uses: `crate::document::NodeKind`
  - uses: `std::hash::{Hash,Hasher}`
  - uses: `device_query::{DeviceQuery,DeviceState,Keycode}`
  - uses: `kurbo::Shape`
  - uses: `crate::tools::StickyPixelMask`
  - uses: `crate::tools::RasterSelectMode`
  - uses: `crate::tools::PaintMaskTool`
  - uses: `std::time::{SystemTime,UNIX_EPOCH}`
  - uses: `std::time::{SystemTime,UNIX_EPOCH}`
  - uses: `std::time::{Duration,Instant}`
  - uses: `base64::Engine`
  - uses: `image::ImageEncoder`
  - uses: `crate::path_physics::PathPhysicsSim`
  - uses: `crate::tools::{WeightFlowStroke,WeightFlowMode}`
  - uses: `glam::Vec2`
  - uses: `crate::document::{linear_angle_from_line,translate_linear_line}`
  - uses: `crate::document::Fill`
  - uses: `base64::Engine`
  - uses: `crate::mcp::node_editorasne`
  - uses: `crate::document::{Fill,Node,NodeKind,Paint,Stroke}`
  - uses: `crate::mcp::drawing::{fill_from_style,parse_color_value,stroke_from_style}`
  - uses: `std::collections::BTreeMap`
  - uses: `crate::document::{ArcJoin,Fill,Node,NodeKind,TextStyle}`
  - uses: `crate::mcp::drawing::{fill_from_style,parse_arc_join,style_from_args,stroke_from_style}`
  - uses: `crate::mcp::drawing::{image_pixel_size,load_image_bytes_from_args}`
  - uses: `crate::mcp::drawing::apply_style_patch`
  - uses: `crate::document::NodeKind`
  - uses: `crate::mcp::drawing::apply_style_patch`
  - uses: `crate::document::{ArcJoin,NodeKind,PathData}`
  - uses: `crate::mcp::drawing::parse_arc_join`
  - uses: `base64::Engine`
  - uses: `kurbo::{BezPath,PathEl,Point}`
  - uses: `crate::mcp::drawing::parse_color_value`
  - uses: `kurbo::{BezPath,Rect}`
  - uses: `super::*`
  - uses: `crate::document::{NodeKind,PathData}`
  - uses: `std::time::{Duration,Instant}`
  - uses: `std::hash::{Hash,Hasher}`
  - uses: `crate::document::AvClip`

### `src/audio_extract.rs`

- **class** `CachedPcm` (L26)
- **class** `AudioPcmCache` (L32)
- **class** `AudioPrepareResult` (L36)
- **class** `ExtractProgress` (L42)
- **function** `extract_global_lock` (L46)
- **function** `extract_once_map` (L55)
- **function** `ensure_extracted_wav` (L64)
- **function** `normalize_wav_path` (L129)
- **function** `extract_audio_to_wav` (L143)
- **function** `extract_audio_to_wav_inner` (L151)
- **function** `wav_is_playable` (L220)
- **function** `stream_file_to_player` (L243)
- **function** `stream_file_to_player_rate` (L253)
- **class** `StreamingWavSource` (L354)
- **method** `open` (L361)
- **class** `Item` (L393)
- **method** `next` (L394)
- **method** `current_span_len` (L406)
- **method** `channels` (L409)
- **method** `sample_rate` (L412)
- **method** `total_duration` (L415)
- **function** `rodio_source_from_path_capped` (L420)
- **function** `rodio_source_from_path_capped_rate` (L428)
- **function** `rodio_source_from_path` (L466)
- **function** `load_pcm_from_wav` (L475)
- **function** `load_pcm_from_file` (L493)
- **function** `load_pcm_decoded` (L520)
- **function** `stereo_pcm_to_cached` (L531)
- **function** `load_pcm_via_rodio` (L544)
- **function** `preload_inflight` (L561)
- **function** `spawn_preload_pcm` (L570)
- **function** `pcm_cache_has` (L619)
- **function** `prepare_samples_at_offset` (L626)
- **function** `slice_cached` (L647)
- **function** `apply_eq_stereo_inplace` (L660)
- **class** `StereoPcmI16` (L693)
- **function** `decode_audio_stereo_symphonia` (L698)
- **function** `decode_audio_stereo_libav` (L788)
- **function** `probe_media_duration_symphonia` (L800)
- **function** `is_audio_track` (L845)
- **function** `write_wav` (L859)
- **function** `write_wav_hound` (L863)
- **function** `resample_interleaved_to_stereo` (L889)
- **function** `append_interleaved_i16` (L921)
- **function** `write_mono_f32_as_wav` (L956)
- **class** `AudioExtractStatus` (L967)
- **method** `is_extracting` (L975)
  - uses: `std::collections::HashMap`
  - uses: `std::fs::File`
  - uses: `std::io::Write`
  - uses: `std::path::{Path,PathBuf}`
  - uses: `std::sync::{Arc,Mutex,OnceLock}`
  - uses: `symphonia::core::audio::{AudioBufferRef,Signal}`
  - uses: `symphonia::core::errors::Error`
  - uses: `symphonia::core::formats::FormatOptions`
  - uses: `symphonia::core::io::MediaSourceStream`
  - uses: `symphonia::core::meta::MetadataOptions`
  - uses: `symphonia::core::probe::Hint`
  - uses: `std::io::Read`
  - uses: `std::num::{NonZeroU16,NonZeroU32}`
  - uses: `rodio::Source`
  - uses: `std::num::{NonZeroU16,NonZeroU32}`
  - uses: `rodio::Source`

### `src/av_ui.rs`

- **class** `AvClipHit` (L15)
- **class** `AvDragMode` (L28)
- **class** `AvTimelineDrag` (L35)
- **class** `PianoTool` (L55)
- **class** `AvTimelineRow` (L65)
- **function** `collect_timeline_rows` (L78)
- **function** `queue_append_start_sec` (L112)
- **function** `av_toolbar` (L125)
- **function** `paint_trim_caps` (L129)
- **function** `hit_test_clip` (L182)
- **function** `apply_sticky_drag` (L224)
- **function** `av_clip_rect` (L260)
- **function** `piano_roll_panel` (L279)
- **function** `music_clip_rect` (L475)
  - uses: `egui::{Color32,Context,Rect,RichText,Ui}`
  - uses: `uuid::Uuid`
  - uses: `crate::app::VadadeeBerryApp`
  - uses: `crate::document::{AvClip,Layer,LayerKind,MusicClip}`
  - uses: `crate::icons`

### `src/bin/vadadee_mcp_stdio.rs`

- **function** `main` (L11)
- **function** `main` (L17)
- **function** `forward_line` (L75)
  - uses: `std::io::{BufRead,BufReader,Write}`
  - uses: `std::net::TcpStream`
  - uses: `std::io::{BufRead,BufReader,Write}`
  - uses: `std::net::TcpStream`

### `src/blend.rs`

- **function** `unpremultiply` (L6)
- **function** `premultiply` (L15)
- **function** `blend_channel` (L19)
- **function** `composite_pixel` (L76)
- **function** `composite_stamp` (L100)
- **function** `document_needs_blend_composite` (L191)
  - uses: `crate::document::BlendMode`

### `src/canvas/mod.rs`

- **class** `Viewport` (L4)
- **method** `default` (L20)
- **method** `zoom_at` (L36)
- **method** `screen_to_doc` (L44)
- **method** `doc_to_screen` (L50)
- **method** `step_x` (L58)
- **method** `step_y` (L67)
- **method** `snap` (L75)
- **method** `page_rect` (L87)
  - uses: `egui::{Pos2,Rect,Vec2}`

### `src/collab/desktop.rs`

- **class** `CollabRole` (L16)
- **class** `CollabConfig` (L23)
- **method** `default` (L35)
- **method** `client_ws_url` (L49)
- **method** `server_bind_addr` (L56)
- **class** `CollabStatus` (L64)
- **class** `CollabEvent` (L74)
- **class** `NetCommand` (L80)
- **class** `CollabUiStateApply` (L89)
- **class** `WirePacket` (L95)
- **class** `CollabSession` (L100)
- **method** `new` (L125)
- **method** `tick_network` (L155)
- **method** `send_ping` (L189)
- **method** `connection_latency_ms` (L202)
- **method** `send_ui_state` (L206)
- **method** `is_connected` (L214)
- **method** `status` (L221)
- **method** `poll` (L225)
- **method** `decrypt_warning_count` (L268)
- **method** `reset_decrypt_warnings` (L272)
- **method** `start` (L277)
- **method** `connect` (L295)
- **method** `disconnect` (L299)
- **method** `take_pending_chat_toasts` (L316)
- **method** `send_chat` (L320)
- **method** `send_cursor` (L337)
- **method** `canvas_outbound_enabled` (L355)
- **method** `enable_canvas_outbound` (L359)
- **method** `take_canvas_push_requested` (L363)
- **method** `set_last_sent_canvas_hash` (L369)
- **method** `send_canvas_if_changed` (L373)
- **method** `broadcast_hello` (L388)
- **method** `send_message` (L396)
- **method** `chat_log` (L405)
- **method** `chat_log_plain` (L409)
- **method** `peers_sorted` (L417)
- **method** `take_pending_canvas_json` (L423)
- **method** `take_pending_ui_state` (L427)
- **method** `apply_remote_message` (L431)
- **function** `color_from_user_id` (L556)
- **function** `fx_hash_str` (L571)
- **function** `spawn_collab_thread` (L578)
- **function** `collab_network_loop` (L594)
- **function** `handle_incoming` (L724)
- **function** `derive_key` (L735)
- **function** `encrypt_message` (L741)
- **function** `decrypt_message` (L758)
  - uses: `std::collections::HashMap`
  - uses: `std::sync::mpsc::{Receiver,Sender,TryRecvError}`
  - uses: `crate::collab::protocol::{ChatLine,CollabMessage,RemotePeer}`
  - uses: `aes_gcm::aead::{Aead,KeyInit}`
  - uses: `aes_gcm::{Aes256Gcm,Nonce}`
  - uses: `base64::Engine`
  - uses: `rand::RngCore`
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `sha2::{Digest,Sha256}`
  - uses: `std::hash::{Hash,Hasher}`
  - uses: `futures_util::{SinkExt,StreamExt}`
  - uses: `std::sync::Arc`
  - uses: `tokio::sync::{mpscastokio_mpsc,Mutex}`
  - uses: `tokio_tungstenite::connect_async`
  - uses: `tokio_tungstenite::tungstenite::Message`

### `src/collab/mod.rs`

- **module** `desktop` (L2)
- **module** `protocol` (L3)
- **module** `relay` (L5)
- **module** `sync_project` (L7)
- **module** `stub` (L11)
  - uses: `protocol::{ChatLine,CollabMessage,RemotePeer}`
  - uses: `desktop::*`
  - uses: `sync_project::*`
  - uses: `stub::*`

### `src/collab/protocol.rs`

- **class** `CollabMessage` (L7)
- **class** `ChatLine` (L55)
- **class** `RemotePeer` (L61)
  - uses: `serde::{Deserialize,Serialize}`

### `src/collab/relay.rs`

- **class** `PeerTx` (L13)
- **class** `Room` (L17)
- **function** `run_relay_until_stopped` (L21)
- **function** `handle_peer` (L52)
  - uses: `std::collections::HashMap`
  - uses: `std::sync::atomic::{AtomicU64,Ordering}`
  - uses: `std::sync::Arc`
  - uses: `futures_util::{SinkExt,StreamExt}`
  - uses: `tokio::net::{TcpListener,TcpStream}`
  - uses: `tokio::sync::{mpsc,Mutex}`
  - uses: `tokio_tungstenite::tungstenite::handshake::server::{Request,Response}`
  - uses: `tokio_tungstenite::{accept_hdr_async,tungstenite::Message}`

### `src/collab/stub.rs`

- **class** `CollabUiStateApply` (L8)
- **class** `CollabRole` (L14)
- **class** `CollabConfig` (L21)
- **method** `default` (L32)
- **method** `client_ws_url` (L46)
- **method** `server_bind_addr` (L50)
- **class** `CollabStatus` (L56)
- **class** `CollabSession` (L64)
- **method** `new` (L72)
- **method** `is_connected` (L81)
- **method** `status` (L85)
- **method** `poll` (L89)
- **method** `tick_network` (L91)
- **method** `decrypt_warning_count` (L93)
- **method** `connection_latency_ms` (L97)
- **method** `start` (L101)
- **method** `connect` (L105)
- **method** `disconnect` (L109)
- **method** `send_chat` (L113)
- **method** `send_cursor` (L124)
- **method** `send_ui_state` (L133)
- **method** `canvas_outbound_enabled` (L135)
- **method** `enable_canvas_outbound` (L139)
- **method** `take_canvas_push_requested` (L141)
- **method** `set_last_sent_canvas_hash` (L145)
- **method** `send_canvas_if_changed` (L147)
- **method** `chat_log` (L149)
- **method** `chat_log_plain` (L153)
- **method** `peers_sorted` (L161)
- **method** `take_pending_canvas_json` (L165)
- **method** `take_pending_ui_state` (L169)
- **method** `take_pending_chat_toasts` (L173)
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `crate::collab::protocol::{ChatLine,RemotePeer}`

### `src/collab/sync_project.rs`

- **function** `asset_sha256_hex` (L9)
- **function** `strip_for_wire` (L16)
- **function** `merge_remote` (L38)
- **function** `hydrate_images` (L49)
- **function** `find_local_image_bytes` (L81)
- **function** `project_wire_hash` (L100)
  - uses: `std::collections::HashMap`
  - uses: `sha2::{Digest,Sha256}`
  - uses: `crate::document::{NodeKind,ProjectFile}`
  - uses: `std::hash::{Hash,Hasher}`

### `src/commands.rs`

- **class** `DocumentChanged` (L30)
- **method** `structure` (L48)
- **method** `nodes` (L58)
- **method** `timeline` (L68)
- **method** `everything` (L76)
- **function** `changes_for_edit` (L90)
- **class** `CommandContext` (L110)
- **class** `EditorCommand` (L121)
- **class** `CommandDispatcher` (L138)
- **method** `dispatch` (L142)
- **module** `tests` (L167)
- **method** `test_ctx` (L172)
- **method** `dispatch_edit_records_undoable_change` (L177)
- **method** `changes_for_edit_maps_variants_to_flags` (L195)
- **method** `dispatch_undo_redo_roundtrip` (L211)
  - uses: `crate::document::ProjectFile`
  - uses: `crate::history::{History,ProjectEdit}`
  - uses: `super::*`
  - uses: `crate::document::Document`
  - uses: `crate::history::snapshot_document`

### `src/cv/jobs.rs`

- **function** `with_sync_cv` (L28)
- **function** `pending` (L35)
- **function** `last_good` (L39)
- **function** `take_dirty` (L44)
- **function** `is_pending` (L48)
- **function** `is_pending_any` (L56)
- **function** `remember_last_good` (L65)
- **function** `last_good_key` (L71)
- **function** `clear_last_good` (L75)
- **function** `ensure_black_placeholder` (L82)
- **class** `JobOutcome` (L96)
- **function** `get_or_schedule` (L108)
- **function** `preview_bake_key` (L154)
- **function** `get_or_run_blocking` (L173)
  - uses: `std::collections::{HashMap,HashSet}`
  - uses: `std::sync::atomic::{AtomicBool,Ordering}`
  - uses: `std::sync::{Mutex,OnceLock}`
  - uses: `super::{global_cv_cache,CvCacheValue}`
  - uses: `uuid::Uuid`

### `src/cv/mod.rs`

- **module** `jobs` (L13)
- **module** `opencv_face` (L14)
- **module** `ops` (L15)
- **module** `track` (L16)
- **class** `CvFaceBackend` (L31)
- **method** `label` (L42)
- **method** `as_u8` (L50)
- **method** `from_u8` (L58)
- **function** `face_backend` (L69)
- **function** `set_face_backend` (L74)
- **function** `detect_faces_auto` (L84)
- **class** `CvRegion` (L132)
- **method** `clamp_norm` (L145)
- **class** `CvMask` (L157)
- **method** `new` (L165)
- **method** `solid` (L177)
- **class** `CvTrackSample` (L189)
- **method** `center_position` (L202)
- **class** `CvPose` (L209)
- **class** `CvCacheValue` (L216)
- **method** `as_mask` (L230)
- **method** `as_regions` (L237)
- **method** `as_track` (L244)
- **method** `as_rgba` (L251)
- **class** `CvJob` (L267)
- **method** `as_str` (L278)
- **class** `CvAnalyzeContext` (L285)
- **method** `time_ms` (L302)
- **method** `cache_key` (L308)
- **function** `hash_params_bytes` (L327)
- **function** `hash_params_f64` (L339)
- **class** `CvCacheInner` (L353)
- **method** `new` (L361)
- **method** `touch` (L370)
- **method** `insert` (L377)
- **method** `get` (L395)
- **class** `CvCache` (L407)
- **method** `new` (L412)
- **method** `insert` (L418)
- **method** `get` (L424)
- **method** `get_rgba_image` (L428)
- **method** `contains` (L434)
- **method** `clear` (L442)
- **method** `clear_keys_containing` (L450)
- **method** `len` (L457)
- **method** `is_empty` (L461)
- **method** `default` (L467)
- **function** `global_cv_cache` (L475)
- **function** `analyze_context_for_media` (L480)
- **module** `tests` (L501)
- **method** `cache_key_stable_for_same_context` (L505)
- **method** `cache_key_differs_on_time_ms_or_params` (L526)
- **method** `cache_roundtrip_mask_and_lru` (L550)
- **method** `hash_params_deterministic` (L577)
- **method** `mask_len_check` (L583)
  - uses: `std::collections::HashMap`
  - uses: `std::sync::{Arc,Mutex,OnceLock}`
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `uuid::Uuid`
  - uses: `super::*`

### `src/cv/opencv_face.rs`

- **function** `detect_faces_opencv` (L14)
- **function** `opencv_available` (L26)
- **function** `cascade_paths` (L38)
- **function** `detect_faces_opencv_inner` (L57)
  - uses: `image::RgbaImage`
  - uses: `super::CvRegion`
  - uses: `opencv::core::{Mat,Size,Vector,CV_8UC1}`
  - uses: `opencv::imgproc`
  - uses: `opencv::objdetect::CascadeClassifier`
  - uses: `opencv::prelude::*`

### `src/cv/ops.rs`

- **function** `rgb_to_hsv` (L8)
- **function** `hue_dist` (L28)
- **function** `chroma_key_mask` (L37)
- **function** `apply_mask_rgba` (L89)
- **function** `feather_mask` (L106)
- **function** `chroma_key_rgba` (L128)
- **function** `background_blur_rgba` (L162)
- **function** `resize_mask_nearest` (L192)
- **function** `detect_face_regions` (L216)
- **function** `rgba_to_cache_value` (L340)
- **function** `pixelate_roi` (L350)
- **function** `blur_roi` (L397)
- **function** `privacy_blur_rgba` (L428)
- **module** `tests` (L473)
- **method** `solid` (L477)
- **method** `green_screen_punches_green_keeps_red` (L486)
- **method** `apply_mask_sets_alpha` (L499)
- **method** `privacy_blur_dims_roi` (L507)
- **method** `background_blur_changes_low_mask` (L538)
  - uses: `super::CvMask`
  - uses: `image::RgbaImage`
  - uses: `super::*`
  - uses: `image::Rgba`

### `src/cv/track.rs`

- **class** `HoldState` (L15)
- **function** `hold_map` (L22)
- **function** `clear_tracker` (L26)
- **function** `clear_all_trackers` (L32)
- **function** `to_gray` (L38)
- **function** `resize_gray` (L50)
- **function** `ncc_best_full` (L70)
- **function** `ncc_at` (L140)
- **function** `track_targets` (L182)
- **module** `tests` (L331)
- **method** `blob_scene` (L335)
- **method** `multi_target_finds_object` (L351)
- **method** `hold_last_when_missing` (L362)
  - uses: `std::collections::HashMap`
  - uses: `std::sync::{Mutex,OnceLock}`
  - uses: `image::RgbaImage`
  - uses: `uuid::Uuid`
  - uses: `super::CvTrackSample`
  - uses: `super::*`
  - uses: `image::Rgba`

### `src/document/animation.rs`

- **class** `InterpolationMode` (L8)
- **method** `default` (L14)
- **function** `default_handle_left` (L19)
- **function** `default_handle_right` (L23)
- **function** `default_handle_mode` (L27)
- **class** `Keyframe` (L32)
- **class** `AnimGraphStackDrag` (L47)
- **function** `solve_u` (L59)
- **class** `KeyframeTrack` (L79)
- **method** `insert` (L84)
- **method** `interpolate` (L100)
- **class** `StackAnimChannel` (L145)
- **class** `StackAnimationFunction` (L159)
- **method** `end_frame` (L168)
- **method** `contains_frame` (L172)
- **method** `local_frame` (L178)
- **method** `t_at` (L183)
- **method** `channel_starts` (L188)
- **method** `vars_for` (L252)
- **method** `sample_channel` (L272)
- **method** `sample_channel_ref` (L295)
- **class** `NodeAnimation` (L315)
- **method** `get_track_mut` (L351)
- **method** `ensure_track` (L387)
- **method** `get_track` (L391)
- **method** `sync_stack_starts_from_keyframes` (L416)
- **method** `ensure_stack_start_keyframes` (L439)
- **method** `sample` (L463)
- **method** `sample_mut` (L473)
- **method** `clear_keyframes_in_open_span` (L486)
- **method** `clear_keyframes_under_stack` (L497)
- **method** `ensure_stack_end_keyframes` (L515)
- **method** `remove_stack_function` (L535)
- **method** `remove_stack_function_with_keyframes` (L543)
- **class** `AnimationTimeline` (L581)
- **module** `param_track_tests` (L586)
- **method** `param_track_insert_and_sample` (L590)
  - uses: `indexmap::IndexMap`
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `uuid::Uuid`
  - uses: `crate::document::{eval_expr_vars,BezierHandleMode,ExprVars,Fill,NodeId}`
  - uses: `super::*`

### `src/document/av_clip.rs`

- **class** `AvClip` (L11)
- **method** `new_from_media` (L29)
- **method** `new_empty` (L43)
- **method** `from_legacy` (L57)
- **method** `is_object_linked` (L79)
- **method** `path_ext` (L83)
- **method** `path_is_audio_only` (L91)
- **method** `path_is_still_image` (L100)
- **method** `path_is_video_container` (L108)
- **method** `path_is_visual_media` (L117)
- **method** `is_audio_only` (L121)
- **method** `is_still_image` (L125)
- **method** `path_fits_video_role` (L130)
- **method** `path_fits_audio_role` (L134)
- **method** `contains_timeline_sec` (L139)
- **method** `timeline_play_secs` (L143)
- **method** `timeline_end_secs` (L156)
- **function** `assign_free_track_row` (L162)
- **function** `ranges_overlap` (L183)
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `uuid::Uuid`

### `src/document/expr.rs`

- **class** `ExprError` (L8)
- **method** `fmt` (L11)
- **class** `ExprVars` (L19)
- **method** `simple` (L33)
- **function** `eval_expr` (L50)
- **function** `eval_expr_vars` (L54)
- **function** `skip_ws` (L75)
- **function** `parse_expr` (L81)
- **function** `parse_add` (L85)
- **function** `parse_mul` (L104)
- **function** `parse_pow` (L136)
- **function** `parse_unary` (L148)
- **function** `parse_primary` (L161)
- **function** `parse_number` (L239)
- **function** `call_fn` (L259)
- **module** `tests` (L327)
- **method** `basic_ops` (L331)
  - uses: `super::*`

### `src/document/flowchart.rs`

- **class** `FlowchartEdgeSide` (L16)
- **class** `FlowchartAnchor` (L24)
- **method** `edge` (L37)
- **method** `edge_at_doc_t` (L47)
- **method** `edge_side` (L57)
- **function** `edge_anchor_t` (L66)
- **class** `FlowchartNodeGeom` (L73)
- **method** `doc_rect` (L82)
- **method** `anchor_position` (L91)
- **method** `contains_doc` (L125)
- **function** `default_endpoint_marker` (L130)
- **function** `default_path_corner` (L134)
- **class** `FlowchartPathData` (L139)
- **function** `edge_inset` (L157)
- **function** `straight_edge_length` (L162)
- **function** `anchor_positions_on_side` (L170)
- **function** `edge_doc_parameter` (L182)
- **function** `slot_for_edge_doc` (L199)
- **function** `distance_to_edge` (L222)
- **function** `nearest_edge_side` (L253)
- **function** `nearest_edge_approach` (L269)
- **function** `anchor_port_normal` (L284)
- **function** `snap_anchor_for_point` (L314)
- **function** `estimated_port_normals` (L332)
- **function** `route_orthogonal` (L360)
- **function** `route_orthogonal_with_normals` (L369)
- **function** `flowchart_routing_obstacles` (L433)
- **function** `flowchart_bend_point_indices` (L450)
- **function** `ensure_three_points` (L477)
- **function** `canonicalize_flowchart_path_points` (L487)
- **function** `orthogonalize_flowchart_path` (L504)
- **function** `fix_flowchart_path_anchor_endpoints` (L508)
- **function** `route_orthogonal_mid` (L534)
- **function** `orthogonal_l_route` (L598)
- **function** `orthogonal_elbow_route` (L616)
- **function** `route_polyline_length` (L627)
- **function** `route_has_inner_crossing` (L633)
- **function** `route_hits_any_obstacle` (L674)
- **function** `bbox_detour_route` (L689)
- **function** `rects_near_equal` (L740)
- **function** `segment_is_horizontal` (L747)
- **function** `segment_is_vertical` (L751)
- **function** `orthogonalize_polyline` (L756)
- **function** `collapse_collinear` (L787)
- **function** `segment_push_route` (L813)
- **function** `segment_intersects_avoid_rect` (L883)
- **function** `segment_intersects_rect_interior` (L908)
- **function** `dedupe_points` (L920)
- **function** `segment_hits_obstacles` (L930)
- **function** `polyline_segments` (L938)
- **function** `flatten_flowchart_stroke` (L942)
- **function** `flowchart_stroke_hit_with_corner` (L955)
- **function** `point_near_segment` (L975)
- **class** `EdgeSortEntry` (L990)
- **function** `edge_sort_key` (L998)
- **function** `rebalance_flowchart_edge_anchors` (L1006)
- **function** `rebalance_flowchart_edge_anchors_with_pending` (L1011)
- **function** `node_as_flowchart_geom` (L1132)
- **function** `new_flowchart_node` (L1152)
- **function** `new_flowchart_node_from_rect` (L1171)
- **function** `sync_flowchart_path_endpoints` (L1192)
- **function** `flowchart_path_jump_points` (L1245)
- **function** `segment_intersection` (L1265)
- **function** `rounded_orthogonal_bez` (L1287)
- **function** `new_flowchart_path` (L1323)
- **module** `routing_tests` (L1341)
- **method** `stacked_nodes_left_right_stubs_avoid_bodies` (L1346)
- **method** `same_node_left_to_top_avoid_body` (L1404)
  - uses: `std::collections::HashMap`
  - uses: `kurbo::{BezPath,PathEl,Point,Rect,Vec2}`
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `super::{Node,NodeId,NodeKind,NodeStore}`
  - uses: `FlowchartEdgeSide::{Bottom,Left,Right,Top}`
  - uses: `super::*`
  - uses: `kurbo::Rect`

### `src/document/mod.rs`

- **module** `node` (L1)
- **module** `path_effects` (L2)
- **module** `style` (L3)
- **module** `animation` (L4)
- **module** `av_clip` (L5)
- **module** `music` (L6)
- **module** `shading` (L7)
- **module** `flowchart` (L8)
- **module** `node_graph` (L9)
- **module** `septic` (L10)
- **module** `expr` (L21)
- **function** `px_to_mm` (L34)
- **function** `mm_to_px` (L38)
- **class** `PageUnit` (L43)
- **class** `Document` (L51)
- **function** `default_page_color` (L82)
- **class** `LayerKind` (L87)
- **class** `AvRole` (L103)
- **method** `default` (L114)
- **class** `Layer` (L120)
- **method** `ne_output_proxy_layer_index` (L214)
- **function** `default_max_duration` (L221)
- **function** `default_zero` (L225)
- **function** `default_one` (L229)
- **function** `default_width` (L233)
- **function** `default_height` (L237)
- **function** `default_true` (L241)
- **function** `default_capture_fps` (L245)
- **function** `default_capture_bitrate_kbps` (L250)
- **method** `new_image` (L255)
- **method** `new_av_layer` (L299)
- **method** `new_empty_av_layer` (L348)
- **method** `new_empty_av_layer_with_role` (L352)
- **method** `new_shading_layer` (L358)
- **method** `new_flowchart_layer` (L405)
- **method** `new_node_editor_layer` (L449)
- **method** `new_screen_record_layer` (L493)
- **method** `ensure_node_graph` (L538)
- **method** `ensure_ne_output_proxy` (L547)
- **method** `ne_output_paint_geom` (L591)
- **method** `fit_ne_output_proxy_to_image` (L645)
- **method** `ensure_av_clips` (L698)
- **method** `has_canvas_video` (L719)
- **method** `shows_video_at` (L733)
- **method** `video_clip_at_time` (L755)
- **method** `sync_legacy_from_clip_id` (L786)
- **method** `sync_legacy_from_primary_clip` (L797)
- **method** `sync_legacy_from_clip_at` (L809)
- **method** `sync_clip_from_legacy` (L822)
- **method** `sync_primary_clip_from_legacy` (L840)
- **method** `prepare_av_for_export` (L849)
- **method** `new_video` (L860)
- **method** `new_audio` (L863)
- **method** `timeline_play_secs` (L868)
- **method** `timeline_end_secs` (L881)
- **function** `default_volume` (L886)
- **function** `default_is_renderer` (L890)
- **method** `new_default_project` (L896)
- **method** `new_empty_project` (L900)
- **method** `page_color_egui` (L920)
- **method** `page_color_svg` (L929)
- **method** `active_layer_mut` (L938)
- **method** `active_layer` (L942)
- **method** `append_to_active_layer` (L946)
- **method** `remove_from_layers` (L952)
- **method** `ordered_node_ids` (L958)
- **method** `add_layer` (L966)
- **method** `add_av_layer` (L972)
- **method** `add_empty_av_layer` (L989)
- **method** `add_empty_av_layer_with_role` (L993)
- **method** `find_av_role_layer` (L1004)
- **method** `ensure_av_role_layer` (L1011)
- **method** `add_shading_layer` (L1027)
- **method** `add_flowchart_layer` (L1035)
- **method** `add_screen_record_layer` (L1041)
- **method** `add_node_editor_layer` (L1047)
- **method** `add_video_layer` (L1054)
- **method** `add_audio_layer` (L1057)
- **method** `move_node_in_active_layer` (L1062)
- **class** `NodeStore` (L1080)
- **method** `insert` (L1085)
- **method** `get` (L1091)
- **method** `get_mut` (L1095)
- **method** `remove` (L1099)
- **class** `ProjectFile` (L1105)
- **method** `new` (L1113)
- **method** `owns_animation_id` (L1122)
- **method** `prune_orphan_animation_tracks` (L1127)
- **method** `remove_node_and_animation` (L1141)
- **module** `p7_proxy_tests` (L1149)
- **method** `ne_project` (L1152)
- **method** `ensure_ne_output_proxy_creates_and_reuses` (L1160)
- **method** `ensure_ne_output_proxy_rebinds_stale_id` (L1179)
- **method** `ensure_ne_output_proxy_reuses_existing_named_image` (L1198)
- **method** `ne_output_proxy_layer_index_and_paint_geom` (L1216)
- **method** `ensure_ne_output_proxy_noop_on_image_layer` (L1257)
  - uses: `av_clip::*`
  - uses: `node::*`
  - uses: `path_effects::*`
  - uses: `style::*`
  - uses: `animation::*`
  - uses: `music::*`
  - uses: `shading::*`
  - uses: `node_graph::*`
  - uses: `septic::*`
  - uses: `expr::{eval_expr,eval_expr_vars,ExprError,ExprVars}`
  - uses: `indexmap::IndexMap`
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `uuid::Uuid`
  - uses: `super::*`

### `src/document/music.rs`

- **class** `MusicNote` (L5)
- **function** `default_velocity` (L13)
- **method** `new` (L18)
- **class** `MusicClip` (L29)
- **method** `new_empty` (L42)
- **method** `end_sec` (L53)
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `uuid::Uuid`

### `src/document/node_graph.rs`

- **class** `PortType` (L10)
- **method** `label` (L38)
- **method** `can_connect` (L56)
- **method** `is_cv_structured` (L70)
- **class** `PortDir` (L76)
- **class** `PortDef` (L82)
- **class** `GraphNodeKind` (L91)
- **function** `default_expr_x` (L214)
- **function** `default_expr_xy` (L217)
- **function** `default_expr_xyz` (L220)
- **function** `default_viz_gain` (L224)
- **function** `union_image_ports` (L229)
- **method** `union_image_slot_count` (L258)
- **method** `is_union_image` (L268)
- **function** `default_mouse_time_threshold` (L272)
- **function** `default_mouse_gain` (L276)
- **method** `category_label` (L282)
- **method** `default_title` (L328)
- **method** `ports` (L376)
- **class** `GraphNode` (L1236)
- **method** `new` (L1248)
- **method** `ports` (L1260)
- **class** `GraphLink` (L1266)
- **method** `new` (L1275)
- **class** `GraphParamKind` (L1287)
- **class** `GraphParam` (L1296)
- **function** `default_one_f64` (L1314)
- **method** `new_real` (L1319)
- **method** `new_color` (L1331)
- **method** `new_position` (L1343)
- **method** `anim_track_labels` (L1357)
- **method** `component_from_track_label` (L1367)
- **class** `GraphImageSource` (L1379)
- **method** `is_empty` (L1394)
- **method** `file_path` (L1399)
- **method** `baked_key` (L1406)
- **class** `GraphSoundSource` (L1416)
- **class** `GraphOutputSound` (L1423)
- **method** `default` (L1440)
- **method** `path` (L1454)
- **class** `GraphOutputEval` (L1464)
- **method** `default` (L1503)
- **method** `needs_pixel_fx` (L1531)
- **method** `only_brightness_fx` (L1541)
- **method** `needs_texture_bake` (L1549)
- **method** `fx_cache_key` (L1558)
- **method** `source_file_path` (L1578)
- **method** `cv_analyze_context` (L1586)
- **method** `with_baked_cache` (L1608)
- **method** `zoom_uv_rect` (L1617)
- **method** `has_zoom` (L1653)
- **method** `media_cache_key` (L1658)
- **method** `quantized_for_cache` (L1667)
- **function** `load_graph_media_rgba` (L1686)
- **function** `load_graph_source_rgba` (L1718)
- **function** `load_graph_eval_rgba` (L1731)
- **function** `materialize_eval_rgba` (L1737)
- **function** `eval_after_spatial_bake` (L1749)
- **function** `continuous_preview_blur_rgba` (L1765)
- **function** `fast_box_blur_rgba` (L1791)
- **function** `box_blur_pass` (L1826)
- **function** `downscale_rgba_max_side` (L1886)
- **function** `export_fast_blur_rgba` (L1900)
- **function** `bake_graph_eval_rgba` (L1921)
- **function** `bake_graph_output_rgba` (L1952)
- **function** `apply_graph_image_fx` (L2045)
- **function** `apply_zoom_crop_export` (L2097)
- **function** `fx_rgb_to_hsl` (L2124)
- **function** `fx_hsl_to_rgb` (L2147)
- **function** `gaussian_kernel` (L2180)
- **function** `gaussian_blur_rgba` (L2202)
- **function** `gaussian_blur_pass` (L2219)
- **class** `GraphView` (L2279)
- **function** `default_zoom` (L2288)
- **method** `default` (L2293)
- **class** `NodeGraph` (L2303)
- **class** `NodeCvKeys` (L2331)
- **method** `default` (L2339)
- **method** `new_empty` (L2345)
- **method** `set_cv_keys` (L2365)
- **method** `cv_keys` (L2369)
- **method** `cached_mask_for_node` (L2374)
- **method** `cached_regions_for_node` (L2381)
- **method** `cached_track_for_node` (L2388)
- **method** `resolve_mask_input` (L2396)
- **method** `resolve_regions_input` (L2417)
- **method** `regions_for_node` (L2430)
- **method** `materialize_privacy_blur` (L2524)
- **method** `chroma_params` (L2603)
- **method** `materialize_chroma_key` (L2629)
- **method** `materialize_apply_mask` (L2658)
- **method** `materialize_background_blur` (L2692)
- **method** `resolve_spatial_effect` (L2727)
- **method** `add_node` (L2772)
- **method** `primary_output_id` (L2785)
- **method** `remove_node` (L2797)
- **method** `sync_parameters_with_nodes` (L2814)
- **method** `port_type` (L2829)
- **method** `try_add_link` (L2837)
- **method** `node_can_reach` (L2887)
- **method** `prune_dead_object_links` (L2918)
- **method** `real_input_source` (L2944)
- **method** `input_source_node` (L2960)
- **method** `resolve_output_image` (L2973)
- **method** `resolve_output_sound` (L3001)
- **method** `resolve_output_sound_for_export` (L3014)
- **method** `resolve_output_sound_source` (L3024)
- **method** `resolve_sound_chain` (L3044)
- **method** `resolve_node_image_out` (L3165)
- **method** `resolve_effect_as_root` (L3249)
- **method** `image_port_dirs` (L3387)
- **method** `collect_union_member_images` (L3406)
- **method** `composite_images_over` (L3447)
- **method** `resolve_union_as_image` (L3490)
- **method** `resolve_image_chain` (L3510)
- **method** `last_real_out` (L3698)
- **method** `last_real_port` (L3703)
- **method** `real_input_value` (L3711)
- **method** `resolve_position_input` (L3723)
- **method** `output_run_till_secs` (L3767)
- **method** `last_time_secs` (L3781)
- **method** `resolve_septic_path` (L3791)
- **method** `eval_reals` (L3822)
- **method** `catalog_kinds_accepting` (L4215)
- **method** `catalog_kinds_producing` (L4272)
- **method** `resolve_septic_player_video` (L4346)
- **method** `resolve_video_player` (L4374)
- **method** `video_player_time_window` (L4394)
- **method** `video_player_media_window` (L4454)
- **module** `tests` (L4493)
- **method** `video_player_sound_with_audio_in_and_time` (L4497)
- **method** `export_fast_blur_changes_pixels` (L4524)
- **method** `bake_graph_output_fallback_missing_file` (L4542)
- **method** `bake_graph_output_fx_cache_hit` (L4558)
- **method** `eval_value_frame_time` (L4585)
- **method** `eval_expr_uses_linked_x` (L4607)
- **method** `type_mismatch_rejected` (L4633)
- **method** `cycle_marks_error` (L4654)
- **method** `catalog_accepts_real` (L4687)
- **method** `param_real_eval` (L4699)
- **method** `eval_chain_frame_into_expr` (L4718)
- **method** `catalog_producing_real_includes_frame` (L4744)
- **method** `expr_error_sets_node_error` (L4754)
- **method** `resolve_output_from_app_object_via_brightness` (L4779)
- **method** `resolve_color_blur_speed_stack` (L4806)
- **method** `export_sound_ignores_run_till_playhead` (L4842)
- **method** `unwired_player_does_not_autoplay_sound` (L4879)
- **method** `resolve_output_sound_via_equalizer` (L4906)
- **method** `apply_graph_image_fx_brightness_darkens` (L4943)
- **method** `apply_graph_image_fx_blur_smooths` (L4953)
- **method** `gaussian_kernel_normalized` (L4971)
- **method** `resolve_output_file_path` (L4982)
- **method** `param_anim_track_labels` (L5001)
- **method** `geometry_nodes_have_control_ports` (L5012)
- **method** `sync_parameters_drops_orphans` (L5033)
- **method** `chroma_key_materializes_mask_and_bake` (L5060)
- **method** `background_blur_node_bakes` (L5114)
- **method** `privacy_blur_with_manual_region` (L5156)
- **method** `detect_face_empty_source` (L5215)
- **method** `detect_face_finds_skin_blob` (L5231)
- **method** `cv_async_schedule_does_not_block` (L5260)
- **method** `track_motion_takes_union_targets` (L5279)
- **method** `union_image_slots_and_cycle_guard` (L5289)
- **method** `collect_union_members_cycle_safe` (L5306)
- **method** `port_type_cv_connect_rules` (L5316)
- **method** `cv_analyze_context_from_eval_uses_media_time` (L5333)
- **method** `load_graph_source_baked_cache_roundtrip` (L5356)
- **method** `node_graph_cv_keys_side_table` (L5380)
- **method** `fx_cache_key_includes_bake` (L5408)
- **method** `resolve_geo_placement_and_size` (L5421)
  - uses: `indexmap::IndexMap`
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `uuid::Uuid`
  - uses: `PortDir::*`
  - uses: `PortType::*`
  - uses: `PortDir::*`
  - uses: `PortType::*`
  - uses: `std::collections::HashSet`
  - uses: `std::collections::{HashSet,VecDeque}`
  - uses: `std::collections::{HashMap,HashSet,VecDeque}`
  - uses: `GraphNodeKind::*`
  - uses: `GraphNodeKind::*`
  - uses: `super::*`

### `src/document/node.rs`

- **class** `NodeId` (L9)
- **class** `PathEditTarget` (L12)
- **method** `anchor_index` (L22)
- **class** `BezierHandleMode` (L31)
- **method** `label` (L48)
- **class** `CornerFillet` (L64)
- **class** `GeometryProfile` (L70)
- **function** `default_font_family` (L159)
- **class** `PlotterRef` (L165)
- **method** `label` (L175)
- **method** `label_parts` (L183)
- **function** `default_plot_expr` (L191)
- **function** `default_plot_domain_min` (L194)
- **function** `default_plot_domain_max` (L197)
- **function** `default_plot_range_min` (L200)
- **function** `default_plot_range_max` (L203)
- **function** `default_true` (L206)
- **function** `default_margin_pct` (L209)
- **function** `default_plot_stroke_width` (L212)
- **function** `default_plot_stroke_rgba` (L215)
- **function** `default_plot_samples` (L218)
- **class** `TextStyle` (L223)
- **method** `default` (L238)
- **method** `has_fixed_width` (L252)
- **method** `wrap_width` (L257)
- **method** `estimate_text_width` (L266)
- **method** `layout_lines` (L281)
- **method** `layout_lines_ex` (L286)
- **class** `NodeKind` (L356)
- **class** `ArcJoin` (L444)
- **class** `TextAlign` (L452)
- **function** `build_arc_bez` (L459)
- **function** `regular_polygon_vertices` (L506)
- **class** `PathData` (L523)
- **method** `from_bez` (L545)
- **method** `handle_mode` (L585)
- **method** `set_handle_mode` (L592)
- **method** `from_anchor_data` (L632)
- **method** `anchor_positions` (L653)
- **method** `is_anchor_smooth` (L657)
- **method** `toggle_anchor_bezier` (L661)
- **method** `set_anchor_smooth` (L666)
- **method** `set_handle_out` (L698)
- **method** `set_handle_in` (L713)
- **method** `apply_handle_drag` (L728)
- **method** `bezier_handles_at` (L794)
- **method** `hit_segment` (L902)
- **method** `segment_nearest_point` (L928)
- **method** `nearest_on_line_segment` (L970)
- **method** `insert_anchor_on_segment` (L990)
- **method** `reverse` (L1021)
- **method** `mirror_horizontal` (L1057)
- **method** `mirror_vertical` (L1077)
- **method** `remove_anchors` (L1097)
- **method** `set_all_anchors_smooth` (L1137)
- **method** `simplify_collinear` (L1149)
- **method** `bezier_handles` (L1182)
- **method** `set_anchor_position` (L1195)
- **method** `move_anchors_by` (L1204)
- **method** `replace_anchors` (L1218)
- **method** `rebuild_with_smooth_anchors` (L1222)
- **method** `to_bez_from_verbs` (L1276)
- **method** `to_bez` (L1319)
- **method** `is_closed` (L1434)
- **method** `set_closed` (L1438)
- **method** `set_corner_fillet` (L1449)
- **method** `clear_corner_fillet` (L1457)
- **method** `get_corner_fillet` (L1461)
- **method** `has_corner_fillet` (L1465)
- **method** `corner_angle_at` (L1471)
- **method** `fillet_tangent_d` (L1505)
- **method** `approximate_length` (L1522)
- **method** `sample_point_and_angle` (L1525)
- **method** `_flatten` (L1530)
- **method** `_approx_len` (L1586)
- **method** `_sample` (L1596)
- **class** `Node` (L1643)
- **class** `Transform2D` (L1655)
- **method** `apply_point` (L1662)
- **interface** `ObjectOnPath` (L1676)
- **interface** `FaceRenderable` (L1680)
- **interface** `PathMagic` (L1698)
- **interface** `Tiling` (L1711)
- **interface** `CircularClone` (L1719)
- **method** `bounds` (L1735)
- **method** `bez_path` (L1736)
- **method** `fill` (L1737)
- **method** `stroke` (L1738)
- **method** `opacity` (L1739)
- **method** `set_opacity` (L1740)
- **method** `translate` (L1741)
- **method** `scale_about_center` (L1742)
- **method** `rotate_about_center` (L1743)
- **method** `clone_renderable` (L1744)
- **method** `as_any` (L1745)
- **method** `to_bez` (L1749)
- **method** `is_closed` (L1756)
- **method** `total_length` (L1763)
- **method** `sample_at` (L1770)
- **method** `clone_path` (L1777)
- **method** `to_bez` (L1781)
- **method** `is_closed` (L1782)
- **method** `total_length` (L1783)
- **method** `sample_at` (L1784)
- **method** `clone_path` (L1785)
- **method** `gaps_for_object` (L1789)
- **method** `gaps_for_object` (L1798)
- **method** `origin` (L1807)
- **method** `set_origin` (L1811)
- **method** `radius` (L1819)
- **method** `set_radius` (L1825)
- **method** `sides` (L1826)
- **method** `set_sides` (L1827)
- **method** `circular_placements` (L1828)
- **method** `origin` (L1842)
- **method** `set_origin` (L1846)
- **method** `radius` (L1857)
- **method** `set_radius` (L1858)
- **method** `sides` (L1859)
- **method** `set_sides` (L1860)
- **method** `circular_placements` (L1861)
- **method** `get_rotation` (L1875)
- **method** `set_rotation` (L1879)
- **method** `get_opacity` (L1898)
- **method** `set_opacity` (L1902)
- **method** `get_color` (L1906)
- **method** `set_color` (L1913)
- **method** `get_stroke_width` (L1917)
- **method** `set_stroke_width` (L1921)
- **method** `get_stroke_color` (L1925)
- **method** `set_stroke_color` (L1936)
- **method** `get_pos` (L1940)
- **method** `new` (L1975)
- **method** `plotter` (L1986)
- **method** `rect` (L2011)
- **method** `ellipse` (L2017)
- **method** `polygon` (L2026)
- **method** `path_from_bez` (L2041)
- **method** `group` (L2045)
- **method** `bounds_with_store` (L2049)
- **method** `is_circle` (L2110)
- **method** `geometry_profile` (L2119)
- **method** `plotter_polyline` (L2288)
- **method** `text` (L2378)
- **method** `image` (L2382)
- **method** `arc` (L2396)
- **method** `line` (L2420)
- **method** `bounds` (L2437)
- **method** `bez_path` (L2510)
- **method** `hit_test_with_store` (L2578)
- **method** `hit_test` (L2595)
- **method** `rotate_about_center` (L2665)
- **method** `scale_about_center` (L2774)
- **method** `translate` (L2845)
- **method** `translate_children` (L2897)
- **method** `set_bounds` (L2908)
- **method** `duplicate` (L2964)
- **method** `flip_h` (L2972)
- **method** `flip_v` (L2979)
- **method** `flip_h_about` (L2986)
- **method** `flip_v_about` (L3044)
- **method** `node_points` (L3100)
- **method** `is_center_edit_handle` (L3105)
- **method** `is_text_origin_handle` (L3117)
- **method** `path_edit_targets` (L3121)
- **method** `apply_path_edit_target` (L3180)
- **method** `edit_handles` (L3236)
- **method** `set_edit_handle` (L3292)
- **method** `get_geom_floats` (L3424)
- **method** `set_geom_floats` (L3517)
- **function** `text_display_name` (L3684)
- **function** `text_bounds` (L3708)
- **function** `text_bounds_rotated` (L3736)
- **function** `image_bounds` (L3742)
- **function** `image_bounds_rotated` (L3747)
- **function** `rect_bounds_rotated` (L3758)
- **function** `image_contains_rotated` (L3789)
- **function** `image_doc_to_uv` (L3820)
- **function** `path_anchor_positions` (L3852)
- **function** `cubic_at` (L3871)
- **function** `unit_vec` (L3889)
- **function** `anchor_tangent` (L3898)
- **function** `segment_controls` (L3941)
- **function** `path_anchor_point_indices` (L3988)
- **module** `bezier_tests` (L4020)
- **method** `flatten_path_points` (L4024)
- **method** `closed_path_anchor_count_stable` (L4037)
- **method** `smooth_anchor_rebuilds_cubic` (L4053)
- **method** `image_rotation_expands_aabb_and_maps_uv` (L4075)
  - uses: `std::collections::HashMap`
  - uses: `kurbo::{BezPath,Rect,Shape}`
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `uuid::Uuid`
  - uses: `super::{Fill,NodeStyle,Paint,Stroke}`
  - uses: `kurbo::PathEl`
  - uses: `kurbo::Shape`
  - uses: `super::*`
  - uses: `kurbo::PathEl`

### `src/document/path_effects.rs`

- **class** `OnPathMode` (L11)
- **class** `ObjectOnPathEffect` (L22)
- **class** `TilingEffect` (L41)
- **method** `default` (L58)
- **class** `CircularRotateMode` (L79)
- **method** `label` (L89)
- **class** `CircularCloneEffect` (L98)
- **method** `default` (L115)
- **method** `ring_radius` (L133)
- **method** `base_angle_rad` (L139)
- **method** `copy_angle_rad` (L146)
- **method** `placement_xy` (L153)
- **method** `instance_rotation_rad` (L165)
- **method** `path_placement` (L174)
- **class** `ClipMaskEffect` (L189)
- **method** `default` (L200)
- **class** `BooleanOpKind` (L212)
- **method** `label` (L223)
- **method** `supports_multi` (L233)
- **class** `BooleanPairMode` (L240)
- **class** `BooleanEffect` (L247)
- **function** `default_true` (L262)
- **method** `default` (L267)
- **function** `is_booleanable_shape` (L280)
- **function** `is_raster_image` (L291)
- **function** `node_to_multipolygon` (L296)
- **function** `compute_boolean_bez` (L399)
- **function** `multipolygon_to_bez` (L422)
- **method** `default` (L457)
- **class** `PathPlacement` (L477)
- **class** `PathSample` (L486)
- **function** `flatten_bez` (L493)
- **function** `build_path_samples` (L560)
- **function** `sample_at` (L592)
- **function** `effect_placements` (L630)
- **function** `default_loft_gap_for_node` (L714)
- **function** `compute_whole_object_bounds` (L723)
- **function** `compute_tiling_whole_bounds` (L745)
- **function** `compute_circular_whole_bounds` (L777)
- **function** `hit_test_circular_clone` (L793)
- **function** `is_pickable_effect_source` (L831)
- **function** `bez_path_from_rect` (L835)
- **function** `build_path_effect_form_node` (L846)
- **function** `sync_path_effect_form_geometry` (L880)
- **function** `path_effect_by_form_node` (L897)
- **function** `path_effect_move_bundle` (L905)
- **function** `path_effect_form_node_ids` (L949)
- **function** `node_uses_extended_pick_bounds` (L956)
- **function** `path_data_for_id` (L968)
- **function** `spatial_index_bounds` (L975)
- **function** `get_effective_bounds` (L987)
- **function** `transform_profile_point` (L1040)
- **function** `profile_points_relative` (L1054)
- **function** `loft_spine_samples` (L1076)
- **function** `loft_sweep_bez` (L1131)
- **function** `loft_sweep_node` (L1229)
- **function** `node_at_placement` (L1247)
- **function** `find_effect_for_pair` (L1297)
- **function** `hidden_effect_sources` (L1308)
- **function** `has_effect_for_objects` (L1316)
- **module** `tests` (L1327)
- **method** `loft_dense_slices_along_open_path` (L1332)
- **method** `loft_sweep_outline_is_single_closed_capsule` (L1353)
- **method** `default_loft_gap_uses_smaller_cross_section` (L1382)
  - uses: `std::collections::HashSet`
  - uses: `indexmap::IndexMap`
  - uses: `kurbo::{BezPath,PathEl,Shape}`
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `uuid::Uuid`
  - uses: `super::{FaceRenderable,Node,NodeId,NodeKind,NodeStore,PathData,PathMagic}`
  - uses: `geo::{Coord,LineString,MultiPolygon,Polygon}`
  - uses: `geo::orient::{Direction,Orient}`
  - uses: `geo::BooleanOps`
  - uses: `geo::{BooleanOps,Coord,LineString,MultiPolygon,Polygon}`
  - uses: `super::*`
  - uses: `crate::document::{Fill,Node,NodeKind,Paint,Stroke}`

### `src/document/septic.rs`

- **class** `MouseSample` (L20)
- **method** `default` (L32)
- **class** `SepticMeta` (L44)
- **function** `default_fps` (L68)
- **function** `default_true` (L71)
- **method** `default` (L76)
- **class** `SepticSession` (L93)
- **class** `SepticCacheEntry` (L100)
- **function** `septic_session_cache` (L106)
- **function** `septic_cache_invalidate` (L111)
- **method** `new_empty` (L119)
- **method** `load_path` (L123)
- **method** `load_path_cached` (L130)
- **method** `load_path_cached_arc` (L136)
- **method** `invalidate_cache` (L168)
- **method** `save_path` (L172)
- **method** `truth_time` (L183)
- **method** `sample_mouse` (L192)
- **method** `sample_event` (L238)
- **method** `mouse_window` (L260)
- **class** `MouseEncoderParams` (L273)
- **method** `default` (L281)
- **class** `MouseEncoderOut` (L290)
- **function** `encode_mouse` (L299)
- **function** `shakiness_from_samples` (L329)
- **function** `resolve_video_path` (L368)
- **module** `tests` (L395)
- **method** `session_with_jitter` (L398)
- **method** `shakiness_higher_on_jitter` (L426)
- **method** `click_event_code` (L459)
- **method** `roundtrip_json` (L488)
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `std::collections::HashMap`
  - uses: `std::path::{Path,PathBuf}`
  - uses: `std::sync::{Arc,Mutex,OnceLock}`
  - uses: `std::time::SystemTime`
  - uses: `super::*`

### `src/document/shading.rs`

- **class** `ShadingStack` (L6)
- **class** `ShadingPass` (L14)
- **method** `clone` (L35)
- **function** `default_enabled` (L50)
- **function** `default_compile_error` (L54)
- **function** `default_hot_reload` (L58)
- **method** `new_preset` (L95)
- **method** `custom_template` (L112)
- **method** `load_wgsl_source` (L119)
- **method** `crt_preset` (L131)
- **method** `vignette_preset` (L138)
- **method** `blackhole_preset` (L145)
- **method** `starfield_preset` (L156)
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `std::sync::{Arc,Mutex}`
  - uses: `uuid::Uuid`

### `src/document/style.rs`

- **class** `Paint` (L4)
- **method** `from_hex` (L9)
- **method** `none` (L18)
- **method** `to_egui` (L24)
- **method** `lerp` (L33)
- **class** `GradientStop` (L47)
- **method** `new` (L53)
- **function** `default_gradient_stops` (L61)
- **function** `default_line_x0` (L68)
- **function** `default_line_y0` (L71)
- **function** `default_line_x1` (L74)
- **function** `default_line_y1` (L77)
- **function** `default_linear_line` (L82)
- **function** `linear_angle_from_line` (L86)
- **function** `project_onto_linear_line` (L90)
- **function** `positive_ray_to_bbox` (L109)
- **function** `linear_line_spanning_bbox` (L137)
- **function** `set_linear_line_angle` (L154)
- **function** `translate_linear_line` (L171)
- **function** `normalize_stops` (L176)
- **function** `sample_stops` (L187)
- **class** `FillKind` (L220)
- **class** `Fill` (L228)
- **method** `default` (L251)
- **method** `none` (L257)
- **method** `is_visible` (L261)
- **method** `kind` (L271)
- **method** `stops` (L279)
- **method** `primary_paint` (L296)
- **method** `secondary_paint` (L300)
- **method** `linear_angle_deg` (L304)
- **method** `radial_center` (L311)
- **method** `linear_line` (L322)
- **method** `sample_at` (L335)
- **method** `build` (L363)
- **class** `LineJoin` (L400)
- **class** `LineCap` (L408)
- **class** `StrokePaintOrder` (L417)
- **method** `label` (L426)
- **class** `MarkerKind` (L435)
- **method** `label` (L451)
- **method** `all` (L462)
- **class** `PathMarker` (L469)
- **method** `default` (L480)
- **class** `Stroke` (L493)
- **method** `default` (L513)
- **class** `BlendMode` (L529)
- **method** `label` (L552)
- **method** `all` (L575)
- **method** `svg_value` (L583)
- **method** `from_label` (L606)
- **class** `NodeStyle` (L633)
- **method** `default` (L642)
  - uses: `serde::{Deserialize,Serialize}`
  - uses: `MarkerKind::*`
  - uses: `BlendMode::*`

### `src/export_audio.rs`

- **class** `ExportAudioLayer` (L11)
- **function** `export_mux_with_audio` (L19)
- **function** `collect_export_audio_layers` (L114)
- **function** `pcm_has_audible_samples` (L210)
- **function** `pcm_peak` (L214)
- **function** `layer_overlaps_export` (L218)
- **function** `resolve_media_path` (L224)
- **function** `mix_timeline_audio_stereo_i16` (L247)
- **function** `lerp_i16_to_f32` (L320)
- **function** `load_stereo_i16_layer` (L332)
- **function** `f32_interleaved_to_stereo_i16` (L349)
- **function** `is_video_container_ext` (L367)
- **module** `export_audio_tests` (L380)
- **method** `prepare_pushes_layer_trim_start_onto_primary_clip` (L387)
- **method** `collect_export_audio_respects_trim_start_and_sorts` (L416)
- **method** `mix_applies_start_offset_if_ozen_present` (L470)
- **method** `mp3_mix_and_aac_roundtrip_if_ozen_present` (L522)
  - uses: `std::path::{Path,PathBuf}`
  - uses: `crate::export_types::VideoFormat`
  - uses: `crate::document::{LayerKind,ProjectFile}`
  - uses: `super::*`
  - uses: `crate::document::{AvClip,Layer,LayerKind,ProjectFile}`
  - uses: `std::path::Path`
  - uses: `uuid::Uuid`

### `src/export_types.rs`

- **class** `VideoBackend` (L13)
- **method** `label` (L20)
- **class** `ExportPowerLevel` (L30)
- **method** `label` (L37)
- **class** `ExportFxQuality` (L50)
- **method** `label` (L61)
- **method** `max_side` (L70)
- **method** `blur_step` (L79)
- **class** `VideoFormat` (L90)
- **method** `label` (L99)
- **method** `extension` (L107)
  - uses: `serde::{Deserialize,Serialize}`

### `src/export_worker.rs`

- **class** `ExportJobConfig` (L28)
- **class** `ExportDurationPlan` (L52)
- **function** `plan_export_duration` (L60)
- **class** `ExportPhase` (L94)
- **class** `ExportWorkerEvent` (L101)
- **function** `spawn_export_worker` (L120)
- **function** `export_cancelled` (L149)
- **class** `ColorAdjust` (L154)
- **method** `active` (L162)
- **class** `ExportVideoLayer` (L170)
- **class** `ExportSession` (L182)
- **method** `new` (L226)
- **method** `bump_ema` (L293)
- **method** `ne_bake_max_side` (L302)
- **method** `ne_blur_step` (L308)
- **method** `needs_full_painter_export` (L315)
- **method** `can_fast_cpu_export` (L334)
- **method** `rasterize_frame_fast_cpu` (L339)
- **method** `ensure_node_image_textures` (L634)
- **method** `ensure_ne_fx_texture` (L673)
- **method** `set_phase` (L714)
- **method** `check_cancel` (L723)
- **method** `prepare` (L731)
- **method** `ensure_encoder` (L742)
- **method** `decode_video_layers` (L774)
- **method** `stream_frame` (L821)
- **method** `finish_encoder` (L829)
- **method** `emit_progress` (L836)
- **method** `note_frame_timing` (L892)
- **method** `encode_all_frames` (L906)
- **method** `finalize` (L1018)
- **method** `abort` (L1062)
- **function** `run_export` (L1072)
- **function** `collect_export_video_layers` (L1120)
- **function** `decode_layer_frame_rgba` (L1183)
- **function** `apply_color_controls` (L1201)
- **function** `rgb_to_hsl` (L1239)
- **function** `hsl_to_rgb` (L1262)
- **function** `apply_animation_for_frame_project` (L1294)
- **function** `apply_node_editor_params_project` (L1508)
- **function** `get_node_geom_floats_project` (L1569)
- **function** `set_node_geom_floats_project` (L1621)
- **method** `rasterize_frame_offscreen_gpu` (L1696)
- **function** `paint_rotated_image` (L2223)
- **module** `duration_plan_tests` (L2259)
- **method** `user_duration_10s_not_clamped_to_content` (L2263)
- **method** `auto_zero_uses_content_length` (L2277)
- **method** `cycles_multiply_total_frames_not_cycle_secs` (L2284)
- **method** `user_duration_shorter_than_one_frame_minimum` (L2292)
- **method** `anim_frame_and_eval_fps_must_agree_on_time` (L2299)
  - uses: `std::path::PathBuf`
  - uses: `std::sync::atomic::{AtomicBool,Ordering}`
  - uses: `std::sync::mpsc::Sender`
  - uses: `std::sync::{Arc,Mutex}`
  - uses: `std::time::Instant`
  - uses: `rustc_hash::FxHashMap`
  - uses: `crate::export_types::{ExportFxQuality,ExportPowerLevel,VideoFormat}`
  - uses: `crate::document::{Fill,NodeId,ProjectFile}`
  - uses: `crate::io::{self,VideoFrameMap,VideoLayerBuffer}`
  - uses: `crate::recorder::{Frame,RecorderConfig,SyncRecorder}`
  - uses: `crate::video_decode::VideoStream`
  - uses: `egui::Context`
  - uses: `resvg::tiny_skia::{Color,Pixmap,PixmapPaint,Transform}`
  - uses: `egui_wgpu::wgpu`
  - uses: `super::plan_export_duration`

### `src/fonts.rs`

- **class** `FontRegistry` (L11)
- **method** `new` (L22)
- **method** `families` (L42)
- **method** `default_family` (L46)
- **method** `query_face_bytes` (L55)
- **method** `family_bound_in` (L78)
- **method** `family_bound_active` (L85)
- **method** `ensure_loaded` (L91)
- **method** `resolved_family` (L176)
- **method** `font_id` (L189)
- **function** `sanitize_svg_font_family` (L197)
- **function** `register_nerd_font_aliases` (L204)
- **function** `build_usvg_fontdb` (L245)
- **function** `usvg_options` (L254)
- **module** `tests` (L262)
- **method** `sanitize_strips_quotes` (L266)
  - uses: `std::collections::HashSet`
  - uses: `std::sync::{Arc,OnceLock}`
  - uses: `egui::{Context,FontData,FontDefinitions,FontFamily,FontId}`
  - uses: `fontdb::{Database,Family,Query}`
  - uses: `crate::icons`
  - uses: `super::*`

### `src/gradient_ui.rs`

- **class** `GradientLineHandle` (L10)
- **class** `GradientEditorFocus` (L19)
- **class** `GradientStripResult` (L26)
- **function** `gradient_strip_editor` (L31)
- **function** `linear_gradient_angle_dial` (L178)
- **function** `line_to_screen` (L209)
- **function** `gradient_flow_line_editor` (L218)
- **function** `norm_in_rect` (L376)
- **function** `apply_angle_to_flow_line` (L384)
- **function** `sync_angle_from_flow_line` (L389)
- **function** `paint_smooth_gradient_bar` (L393)
- **function** `solid_color_editor` (L412)
- **function** `paint_kind_selector` (L438)
  - uses: `egui::{Color32,Id,Key,Mesh,Pos2,Rect,Sense,Shape,Stroke,Ui,Vec2}`
  - uses: `crate::theme::colors`

### `src/history.rs`

- **class** `ProjectEdit` (L10)
- **class** `Target` (L52)
- **class** `Output` (L53)
- **method** `edit` (L55)
- **method** `undo` (L59)
- **class** `History` (L64)
- **method** `default` (L71)
- **method** `with_limit` (L82)
- **method** `revision` (L89)
- **method** `clear` (L95)
- **method** `push` (L100)
- **method** `push_applied` (L106)
- **method** `undo` (L111)
- **method** `redo` (L120)
- **method** `can_undo` (L129)
- **method** `can_redo` (L133)
- **method** `limit` (L138)
- **function** `apply_forward` (L143)
- **function** `apply_inverse` (L221)
- **function** `snapshot_project` (L296)
- **function** `snapshot_document` (L300)
- **module** `p7_proxy_history_tests` (L305)
- **method** `remove_nodes_clears_and_restores_ne_output_proxy` (L310)
- **method** `export_fx_quality_levels` (L350)
  - uses: `undo::{Edit,Record}`
  - uses: `super::*`
  - uses: `crate::document::Document`
  - uses: `crate::export_types::ExportFxQuality`

### `src/icons.rs`

- **function** `nerd_font_id` (L93)
- **function** `polygon_icon` (L97)

### `src/io.rs`

- **class** `VideoLayerBuffer` (L14)
- **class** `VideoFrameMap` (L20)
- **function** `default_project_filename` (L26)
- **class** `IoError` (L47)
- **function** `load_project` (L52)
- **function** `save_project` (L57)
- **function** `import_svg` (L62)
- **function** `path_from_usvg` (L120)
- **function** `export_svg` (L172)
- **function** `document_svg_string` (L177)
- **function** `node_svg_for_bounds` (L330)
- **function** `node_to_svg_fragment` (L347)
- **function** `stops_svg` (L528)
- **function** `fill_svg` (L542)
- **function** `stroke_join_attr` (L574)
- **function** `stroke_cap_attr` (L582)
- **function** `stroke_svg` (L590)
- **function** `stop_attr` (L643)
- **function** `paint_attr` (L653)
- **function** `path_to_svg_d` (L663)
- **function** `selection_paint_order` (L670)
- **function** `export_selected_svg_string` (L680)
- **function** `rasterize_selection_rgba` (L701)
- **class** `ExportImageFormat` (L715)
- **method** `label` (L724)
- **method** `extension` (L733)
- **function** `write_image_file` (L743)
- **function** `export_document_raster` (L783)
- **function** `export_selection_raster` (L795)
- **function** `document_svg_for_view` (L811)
- **function** `default_document_view` (L834)
- **function** `resolve_capture_view` (L843)
- **function** `rasterize_document_view` (L862)
- **function** `render_svg_to_rgba` (L919)
- **function** `render_svg_to_rgba_even` (L939)
- **function** `layer_anim_transform` (L969)
- **function** `video_layer_dest_size` (L995)
- **function** `clip_defs_and_maps` (L1013)
- **function** `append_image_layer_nodes_to_svg` (L1048)
- **function** `document_svg_single_image_layer` (L1076)
- **function** `document_svg_nodes_only` (L1096)
- **function** `rasterize_image_layer` (L1142)
- **function** `composite_export_frame` (L1153)
- **function** `apply_shading_passes_skia_public` (L1317)
- **function** `apply_shading_passes_skia` (L1325)
- **function** `apply_vignette_pixels` (L1448)
- **module** `tests` (L1474)
- **method** `path_to_svg_d_includes_cubic_segments` (L1478)
- **method** `text_svg_export_includes_transform_rotation` (L1503)
- **method** `text_rotation_survives_svg_raster_export_path` (L1527)
- **function** `raster_text` (L1531)
  - uses: `std::fs`
  - uses: `std::path::Path`
  - uses: `kurbo::BezPath`
  - uses: `thiserror::Error`
  - uses: `usvg::tiny_skia_path::PathSegment`
  - uses: `base64::Engine`
  - uses: `base64::Engine`
  - uses: `kurbo::Rect`
  - uses: `resvg::tiny_skia::{Color,Pixmap,PixmapPaint,Transform}`
  - uses: `super::*`
  - uses: `crate::document::{Node,NodeStore,TextStyle}`
  - uses: `crate::document::{Fill,Node,NodeStore,Paint,TextStyle}`
  - uses: `resvg::tiny_skia::{Pixmap,Transform}`

### `src/layer_cache.rs`

- **class** `LayerRasterCacheEntry` (L20)
- **class** `LayerCacheResult` (L26)
- **class** `RectItem` (L36)
- **function** `layer_has_blend_nodes` (L44)
- **function** `layer_has_animated_nodes` (L61)
- **function** `layer_has_text_nodes` (L68)
- **function** `should_cache_layer` (L84)
- **function** `cache_entry_valid` (L124)
- **function** `layer_is_solid_rect_only` (L132)
- **function** `fill_to_skia_color` (L158)
- **function** `collect_rect_items` (L174)
- **function** `rasterize_rect_layer_parallel` (L208)
- **function** `spawn_layer_raster_job` (L285)
- **function** `install_cache_result` (L326)
  - uses: `std::collections::{HashMap,HashSet}`
  - uses: `std::sync::mpsc`
  - uses: `egui::{ColorImage,TextureHandle,TextureOptions}`
  - uses: `rayon::prelude::*`
  - uses: `resvg::tiny_skia::{Color,RectasSkRect}`
  - uses: `crate::document::{Fill,Layer,LayerKind,NodeId,NodeKind,ProjectFile}`
  - uses: `crate::document::BlendMode`
  - uses: `crate::document::{BlendMode,Fill}`

### `src/left_dock.rs`

- **class** `LeftDockPanel` (L16)
- **class** `ChatToast` (L22)
- **class** `LeftDockState` (L28)
- **method** `push_chat_toast` (L38)
- **method** `tick_toasts` (L53)
- **method** `toggle` (L63)
- **method** `default` (L76)
- **function** `show` (L88)
- **function** `chat_body` (L128)
- **function** `collab_body` (L176)
- **function** `server_setup_panel` (L263)
- **function** `mcp_preview_panel` (L288)
- **function** `mcp_setup_hint` (L320)
- **function** `mcp_cursor_config_json` (L337)
- **function** `copyable_config_block` (L353)
- **function** `peers_panel` (L372)
- **function** `truncate_fit` (L430)
- **function** `peer_latency_label` (L439)
- **function** `show_chat_toasts` (L452)
- **function** `collab_status_label` (L497)
- **function** `draw_local_cursor_bubble` (L524)
- **function** `draw_remote_cursors` (L576)
- **function** `collab_display_color` (L626)
- **function** `draw_cursor_bubble_label` (L630)
- **function** `draw_pointer_cursor` (L656)
- **function** `draw_outlined_text` (L670)
- **function** `draw_outlined_icon` (L684)
- **function** `tool_kind_from_label` (L698)
- **interface** `DrawingTool` (L722)
- **method** `is_drawing_tool` (L727)
- **function** `collab_tool_icon` (L749)
  - uses: `egui::scroll_area::ScrollBarVisibility`
  - uses: `egui::{Context,FontId,Rect,RichText,ScrollArea,Ui}`
  - uses: `crate::animation::left_dock_panel_rect`
  - uses: `crate::app::VadadeeBerryApp`
  - uses: `crate::collab::RemotePeer`
  - uses: `crate::icons`
  - uses: `crate::theme::{self,colors}`
  - uses: `crate::tools::ToolKind`

### `src/lib.rs`

- **module** `av_ui` (L3)
- **module** `node_editor_ui` (L4)
- **module** `animation` (L5)
- **module** `blend` (L6)
- **module** `app` (L7)
- **module** `canvas` (L8)
- **module** `commands` (L9)
- **module** `document` (L10)
- **module** `fonts` (L11)
- **module** `gradient_ui` (L12)
- **module** `history` (L13)
- **module** `icons` (L14)
- **module** `io` (L15)
- **module** `layer_cache` (L16)
- **module** `selection` (L17)
- **module** `state` (L18)
- **module** `perf` (L19)
- **module** `shading` (L20)
- **module** `spatial_index` (L21)
- **module** `left_dock` (L22)
- **module** `render` (L23)
- **module** `text_glyph` (L24)
- **module** `theme` (L25)
- **module** `tools` (L26)
- **module** `raster` (L27)
- **module** `path_physics` (L28)
- **module** `ui` (L29)
- **module** `video_decode` (L30)
- **module** `cv` (L31)
- **module** `export_worker` (L32)
- **module** `export_audio` (L33)
- **module** `export_types` (L34)
- **module** `recorder` (L35)
- **module** `audio_extract` (L36)
- **module** `screen_capture` (L38)
- **module** `collab` (L39)
- **module** `sys_stats` (L40)
- **module** `mcp` (L42)
- **function** `app_icon` (L51)
- **function** `native_options` (L63)
- **function** `init_logging` (L88)
- **function** `run_desktop` (L122)
- **function** `android_main` (L136)
  - uses: `app::VadadeeBerryApp`

### `src/main.rs`

- **function** `main` (L7)
- **function** `main` (L12)

### `src/mcp/drawing.rs`

- **class** `McpShapeStyle` (L8)
- **function** `style_props_schema` (L17)
- **function** `style_from_args` (L34)
- **function** `default_fill` (L66)
- **function** `fill_from_style` (L70)
- **function** `stroke_from_style` (L77)
- **function** `apply_style_patch` (L89)
- **function** `apply_marker_patch` (L179)
- **function** `parse_color_value` (L215)
- **function** `load_image_bytes_from_args` (L231)
- **function** `image_pixel_size` (L299)
- **function** `parse_arc_join` (L304)
- **function** `drawing_tools` (L312)
- **function** `merge_props` (L909)
- **function** `tool` (L921)
  - uses: `serde_json::{json,Value}`
  - uses: `crate::document::{Fill,Paint,Stroke}`
  - uses: `base64::Engineas_`

### `src/mcp/mod.rs`

- **module** `drawing` (L12)
- **module** `node_editor` (L13)
- **module** `path_parse` (L14)
- **class** `McpAppSnapshot` (L20)
- **class** `McpHostRequest` (L31)
- **class** `McpHostResponse` (L63)
- **class** `McpShared` (L74)
- **class** `McpBridge` (L78)
- **method** `try_start` (L84)
- **method** `start` (L109)
- **method** `drain_pending` (L122)
- **function** `mcp_tcp_server` (L140)
- **function** `handle_mcp_client` (L156)
- **class** `JsonRpcRequest` (L188)
- **function** `empty_tool_schema` (L197)
- **function** `mcp_tool` (L201)
- **function** `mcp_initialize_result` (L209)
- **function** `mcp_tools_list_result` (L217)
- **function** `try_answer_stdio_line` (L331)
- **function** `handle_jsonrpc` (L353)
- **function** `is_drawing_tool` (L465)
- **function** `is_mcp_notification` (L512)
- **function** `host_call` (L516)
  - uses: `std::io::{BufRead,BufReader,Write}`
  - uses: `std::net::{TcpListener,TcpStream}`
  - uses: `std::sync::mpsc::{Receiver,Sender,TryRecvError}`
  - uses: `std::sync::Arc`
  - uses: `std::thread::JoinHandle`
  - uses: `serde::Deserialize`
  - uses: `serde_json::{json,Value}`

### `src/mcp/node_editor.rs`

- **function** `node_editor_tools` (L9)
- **function** `tool` (L148)
- **function** `is_node_editor_tool` (L160)
- **function** `kind_from_args` (L178)
- **function** `resolve_port_id` (L306)
- **function** `parse_uuid_list` (L391)
- **function** `kind_label` (L401)
- **function** `node_fields_json` (L449)
- **function** `ports_json` (L477)
- **function** `attach_param` (L496)
- **function** `list_kinds_json` (L531)
  - uses: `serde_json::{json,Value}`
  - uses: `uuid::Uuid`
  - uses: `crate::document::{GraphNodeKind,GraphParam,PortDir}`

### `src/mcp/path_parse.rs`

- **function** `bez_from_svg_d` (L5)
- **function** `next_num` (L15)
- **function** `tokenize` (L102)
  - uses: `kurbo::{BezPath,PathEl}`

### `src/node_editor_ui.rs`

- **function** `port_type_color` (L20)
- **function** `node_width` (L38)
- **function** `aspect_fit_rect` (L46)
- **class** `NodeEditorToolMode` (L60)
- **class** `NodeEditorUiState` (L74)
- **method** `default` (L98)
- **method** `open` (L117)
- **method** `close` (L129)
- **method** `can_edit` (L143)
- **function** `show_node_editor_dialog` (L148)
- **function** `show_object_from_app_picker` (L265)
- **function** `node_editor_mode_keys` (L382)
- **function** `node_editor_toolbar` (L435)
- **function** `show_tool_overlay` (L547)
- **function** `add_menu_strip` (L569)
- **function** `view_menu_strip` (L911)
- **function** `spawn_node_centered` (L939)
- **function** `fit_graph_view` (L952)
- **function** `node_height` (L990)
- **function** `flat_text_button` (L1027)
- **function** `node_editor_canvas` (L1039)
- **function** `truncate_middle` (L2202)
- **function** `paint_grid` (L2221)
- **function** `graph_to_screen` (L2246)
- **function** `screen_to_graph` (L2252)
- **function** `port_screen_pos` (L2259)
- **function** `port_rect_for` (L2273)
- **function** `paint_mouse_xy_preview` (L2306)
- **function** `muted_axis_label` (L2442)
- **function** `paint_node_card` (L2446)
- **function** `wire_screen_points` (L2526)
- **function** `dist_point_to_polyline` (L2555)
- **function** `dist_point_to_segment` (L2566)
- **function** `paint_wire_flowchart` (L2579)
- **function** `paint_polyline_curved` (L2594)
- **function** `paint_quarter_arc` (L2660)
- **function** `parameter_tab_ui` (L2701)
  - uses: `egui::{Color32,Context,Pos2,Rect,RichText,Sense,Ui,Vec2}`
  - uses: `uuid::Uuid`
  - uses: `crate::app::VadadeeBerryApp`
  - uses: `crate::icons::{self,nerd_font_id}`
  - uses: `crate::theme::colors`

### `src/path_physics.rs`

- **class** `PathPhysicsSim` (L14)
- **method** `from_anchors` (L25)
- **method** `to_anchors` (L55)
- **method** `step` (L63)
- **function** `falloff` (L225)
  - uses: `glam::Vec2`
  - uses: `crate::tools::weight_flow::{Falloff,MagneticPole,WeightFlowConfig,WeightFlowMode}`

### `src/perf.rs`

- _no tagged symbols_

### `src/raster/mod.rs`

- **class** `RasterBuffer` (L10)
- **method** `new` (L17)
- **method** `from_rgba` (L28)
- **method** `from_png_bytes` (L42)
- **method** `encode_png` (L53)
- **method** `transparent_png` (L66)
- **method** `stamp_circle` (L71)
- **method** `stamp_circle_clipped` (L88)
- **method** `stamp_circle_clipped_poly` (L117)
- **method** `stamp_circle_masked` (L146)
- **method** `stamp_tip_masked` (L183)
- **method** `smudge_circle` (L366)
- **function** `brush_unit_noise` (L462)
- **function** `stamps_along` (L473)
- **function** `catmull_rom` (L511)
- **function** `stamps_along_catmull` (L523)
- **function** `stamps_for_new_sample` (L604)
- **function** `dilate_mask` (L644)
- **function** `dilate_mask_separable` (L694)
- **function** `point_in_polygon` (L738)
- **function** `expand_circular_symmetry` (L761)
- **function** `flood_fill` (L790)
- **function** `color_match` (L855)
- **function** `doc_size_to_pixel_radius` (L863)
- **module** `tests` (L885)
- **method** `stamp_paints_opaque_pixel` (L889)
- **method** `alpha_lock_skips_transparent` (L898)
- **method** `erase_clears_alpha` (L931)
- **method** `circular_symmetry_duplicates_stamps` (L940)
- **method** `continuous_stamps_fill_gap` (L949)
- **method** `flood_fill_fills_region` (L956)
- **method** `flood_fill_does_not_select_inner_hole` (L975)
- **method** `catmull_spiral_segment_is_not_just_chord` (L1001)
- **method** `png_roundtrip` (L1014)
  - uses: `image::ImageEncoder`
  - uses: `super::*`

### `src/recorder/async_bridge.rs`

- **class** `BridgeMode` (L11)
- **class** `AsyncBridge` (L24)
- **method** `new` (L29)
- **method** `is_recording` (L35)
- **method** `is_async` (L39)
- **method** `start_sync` (L44)
- **method** `start_recording` (L64)
- **method** `start_recording_with_depth` (L69)
- **method** `write_frame` (L92)
- **method** `stop_recording` (L103)
- **method** `frame_sender` (L118)
- **method** `into_config` (L126)
- **function** `dummy_config` (L134)
  - uses: `std::sync::mpsc::{self,SyncSender}`
  - uses: `super::async_recorder::AsyncRecorder`
  - uses: `super::sync::{Frame,RecorderConfig,SyncRecorder}`

### `src/recorder/async_recorder.rs`

- **class** `AsyncRecorder` (L9)
- **method** `spawn` (L15)
- **method** `join` (L24)
- **method** `drop` (L35)
- **function** `run_encoder` (L48)
  - uses: `std::sync::mpsc::Receiver`
  - uses: `std::thread::{self,JoinHandle}`
  - uses: `super::sync::{Frame,RecorderConfig,SyncRecorder}`

### `src/recorder/mod.rs`

- **module** `async_bridge` (L3)
- **module** `async_recorder` (L4)
- **module** `sync` (L5)
  - uses: `async_bridge::AsyncBridge`
  - uses: `async_recorder::AsyncRecorder`
  - uses: `sync::{Frame,RecorderConfig,SyncRecorder}`

### `src/recorder/sync.rs`

- **class** `RecorderConfig` (L9)
- **method** `output_path_str` (L21)
- **class** `Frame` (L30)
- **method** `new` (L37)
- **method** `from_parts` (L45)
- **method** `validate` (L51)
- **class** `SyncRecorder` (L70)
- **method** `start` (L77)
- **method** `write_frame` (L98)
- **method** `finish` (L109)
  - uses: `std::path::PathBuf`
  - uses: `crate::video_decode::LibavEncoder`

### `src/render.rs`

- **function** `path_flatten_tolerance` (L25)
- **function** `fill_flatten_tolerance` (L33)
- **function** `draw_grid` (L37)
- **function** `draw_page_shadow` (L108)
- **function** `paint_to_color` (L115)
- **function** `stroke_width` (L127)
- **function** `sample_fill_at` (L134)
- **function** `sample_paint_fill` (L141)
- **function** `draw_gradient_line` (L145)
- **function** `lerp_u8` (L169)
- **function** `draw_stroke_closed_ring` (L173)
- **function** `stroke_cap_circles` (L229)
- **function** `stroke_join_dots` (L238)
- **function** `segment_endpoints_for_join` (L254)
- **function** `draw_stroke_open_polyline` (L265)
- **function** `rounded_rect_path_points` (L315)
- **function** `draw_rect_stroke` (L337)
- **function** `draw_ellipse_stroke` (L417)
- **function** `doc_norm` (L452)
- **function** `screen_norm` (L458)
- **function** `lyon_fill_options` (L464)
- **function** `to_lyon_line_join` (L471)
- **function** `to_lyon_line_cap` (L479)
- **function** `draw_solid_bez_stroke` (L488)
- **function** `stroke_bez_lyon_mesh` (L521)
- **function** `paint_stroke_mesh_with_aa` (L568)
- **function** `bez_to_lyon_path_for_stroke` (L582)
- **function** `draw_feathered_polyline_stroke` (L641)
- **function** `bez_to_lyon_path` (L676)
- **function** `ellipse_bez_path` (L731)
- **function** `ellipse_ring_points` (L735)
- **function** `flatten_path_points` (L753)
- **function** `doc_bounds_screen_rect` (L768)
- **function** `clipped_gradient_mesh_from_bez` (L776)
- **function** `polygon_bez_path` (L798)
- **function** `rounded_rect_gradient_mesh` (L811)
- **function** `rect_gradient_mesh` (L829)
- **function** `linear_gradient_rect_bands` (L874)
- **class** `GradVert` (L1173)
- **function** `fill_param_at` (L1179)
- **function** `gradient_cut_levels` (L1201)
- **function** `lerp_grad_vert` (L1219)
- **function** `split_triangle_at_t` (L1233)
- **function** `subdivide_triangle_for_stops` (L1308)
- **function** `emit_grad_triangle` (L1326)
- **function** `tessellate_clipped_gradient` (L1348)
- **function** `add_clipped_gradient_mesh` (L1426)
- **function** `draw_shape_fill` (L1468)
- **function** `doc_to_screen_pos` (L1518)
- **function** `bez_to_feathered_stroke_shapes` (L1523)
- **function** `bez_to_fill_shapes` (L1612)
- **function** `bez_to_egui_shapes` (L1665)
- **function** `polyline_from_bez` (L1692)
- **function** `draw_node` (L1708)
- **function** `selection_screen_rect` (L2346)
- **function** `selection_union_screen_rect` (L2367)
- **function** `draw_group_selection_bounds` (L2436)
- **function** `draw_transform_handles` (L2445)
- **function** `handle_positions` (L2468)
- **function** `hit_resize_handle` (L2481)
- **function** `draw_nodes` (L2505)
- **function** `draw_nodes_ex` (L2538)
- **class** `BlendRoiCache` (L2649)
- **function** `blend_content_key` (L2654)
- **function** `draw_nodes_with_blend` (L2688)
- **function** `rebuild_blend_roi_tex` (L2848)
- **function** `stamp_node_into_blend_roi` (L2931)
- **function** `rasterize_node_region` (L2985)
- **function** `decode_image_cached` (L3109)
- **function** `crop_rgba` (L3140)
- **function** `resize_rgba` (L3162)
- **function** `draw_tiling_effects` (L3187)
- **function** `draw_circular_effects` (L3245)
- **function** `draw_path_effects` (L3287)
- **function** `marker_local_points` (L3368)
- **function** `transform_to_screen` (L3395)
- **function** `draw_one_marker` (L3408)
- **function** `get_marker_placements` (L3473)
- **function** `draw_path_markers` (L3574)
- **function** `draw_preview_rect` (L3605)
- **function** `draw_marquee_rect` (L3626)
- **function** `draw_preview_ellipse` (L3646)
- **function** `draw_preview_polygon` (L3667)
- **function** `append_smoothed_points` (L3687)
- **function** `draw_pixel_brush_preview` (L3723)
- **function** `draw_weight_flow_cursor` (L3783)
- **function** `draw_brush_preview` (L3842)
- **function** `draw_preview_line` (L4059)
- **function** `draw_preview_bezier` (L4076)
- **function** `draw_pen_preview` (L4138)
- **function** `text_font_id` (L4202)
- **function** `paint_image_rotated` (L4210)
- **function** `draw_text_node` (L4263)
- **function** `draw_node_handles` (L4324)
- **function** `gradient_line_screen` (L4499)
- **function** `paint_to_overlay_color` (L4515)
- **function** `draw_stop_markers_on_line` (L4525)
- **function** `draw_gradient_flow_overlay` (L4544)
- **function** `pick_gradient_flow_handle` (L4611)
- **function** `radial_from_bounds_drag` (L4653)
- **function** `linear_norm_from_bounds_drag` (L4662)
- **function** `draw_eyedropper_magnifier` (L4665)
- **module** `lyon_path_tests` (L4786)
- **method** `screen_polyline_to_lyon_path` (L4794)
- **method** `assert_stroke_tessellates` (L4811)
- **method** `open_pen_path_stroke` (L4827)
- **method** `closed_path_stroke_no_duplicate_close` (L4839)
- **method** `closed_path_via_set_closed_stroke` (L4859)
- **method** `smooth_closed_path_stroke` (L4872)
- **method** `bez_with_consecutive_close_paths` (L4884)
- **method** `screen_polyline_closed_ring` (L4895)
- **function** `draw_clip_mask_effects` (L4916)
- **function** `clip_image_mesh` (L5005)
- **function** `draw_dashed_polyline` (L5097)
  - uses: `egui::{Align2,Color32,FontFamily,FontId,Mesh,Painter,Pos2,Rect,Shape,Stroke,Vec2}`
  - uses: `kurbo::{BezPath,Ellipse,PathEl,RectasKurboRect,ShapeasKurboShape}`
  - uses: `lyon::math::Point`
  - uses: `lyon::path::Path`
  - uses: `crate::canvas::Viewport`
  - uses: `std::collections::HashSet`
  - uses: `crate::document::StrokeasDocStroke`
  - uses: `crate::theme::colors`
  - uses: `crate::gradient_ui::GradientLineHandle`
  - uses: `crate::tools::ResizeHandle`
  - uses: `std::hash::{Hash,Hasher}`
  - uses: `std::collections::HashMap`
  - uses: `std::hash::{Hash,Hasher}`
  - uses: `std::sync::Mutex`
  - uses: `crate::document::{FaceRenderable,node_at_placement}`
  - uses: `crate::document::{FaceRenderable,node_at_placement}`
  - uses: `crate::tools::{MagneticPole,WeightFlowMode}`
  - uses: `super::*`
  - uses: `crate::document::PathData`
  - uses: `std::collections::HashMap`
  - uses: `crate::document::node_to_multipolygon`
  - uses: `geo::BooleanOps`
  - uses: `geo::{Coord,LineString,MultiPolygon,Polygon}`

### `src/screen_capture.rs`

- **class** `CaptureClock` (L35)
- **method** `new` (L48)
- **method** `set_fps` (L58)
- **method** `mark_video_start` (L63)
- **method** `note_frame_written` (L80)
- **method** `is_running` (L87)
- **method** `media_sec` (L96)
- **method** `last_frame_media_sec` (L116)
- **method** `set_video_duration` (L126)
- **method** `video_duration` (L132)
- **class** `LivePointer` (L139)
- **method** `default` (L148)
- **class** `ScreenCaptureSession` (L159)
- **class** `CaptureBackend` (L183)
- **class** `ScreenCaptureStart` (L189)
- **function** `resolve_bitrate_kbps` (L201)
- **function** `push_app_pointer` (L210)
- **function** `clear_app_pointer` (L212)
- **function** `is_wayland_session` (L214)
- **method** `start` (L222)
- **method** `elapsed_sec` (L238)
- **method** `sample_count` (L242)
- **method** `stop` (L246)
- **function** `stop_ffmpeg_child` (L300)
- **class** `SystemAudioCapture` (L334)
- **method** `start` (L342)
- **method** `finish` (L374)
- **function** `run_pipewire_audio_capture` (L399)
- **class** `AudioData` (L409)
- **function** `run_pipewire_audio_capture` (L644)
- **function** `mux_pcm_into_video_libav` (L653)
- **function** `finalize_session` (L684)
- **method** `drop` (L776)
- **function** `unix_kill` (L787)
- **function** `unix_fcntl` (L797)
- **function** `spawn_mouse_thread` (L811)
- **function** `push_mouse_at_frame` (L931)
- **class** `MouseTracker` (L963)
- **method** `new` (L986)
- **method** `latch` (L1058)
- **method** `report` (L1068)
- **method** `at_edge` (L1076)
- **method** `push_abs_hist` (L1081)
- **method** `abs_is_moving` (L1090)
- **method** `poll` (L1102)
- **method** `poll_evdev_deltas` (L1209)
- **function** `poll_global_pointer_capture` (L1278)
- **function** `is_likely_frozen_mid` (L1309)
- **function** `map_abs_px_to_capture` (L1314)
- **function** `probe_x11_screen_size` (L1363)
- **function** `sibling_video_path` (L1386)
- **function** `safe_layer_stem` (L1390)
- **function** `default_capture_dir` (L1409)
- **function** `default_sepscrr_path` (L1414)
- **function** `sepscrr_in_dir` (L1419)
- **function** `resolve_sepscrr_for_record` (L1429)
- **function** `dirs_screen_capture_dir` (L1447)
- **function** `probe_screen_size` (L1462)
- **function** `start_x11grab` (L1493)
- **function** `portal_block_on` (L1599)
- **function** `scale_rgba` (L1606)
- **function** `cap_encode_size` (L1617)
- **class** `PwFrameSlot` (L1628)
- **class** `PortalCursor` (L1643)
- **class** `PortalPwRemote` (L1654)
- **function** `open_screencast_remote` (L1664)
- **function** `pw_buffer_to_rgba` (L1753)
- **function** `extract_spa_meta_cursor` (L1860)
- **function** `run_pipewire_capture` (L1901)
- **class** `UserData` (L1913)
- **function** `start_wayland_portal_rust` (L2157)
- **function** `start_wayland_portal_rust` (L2419)
  - uses: `std::io::Read`
  - uses: `std::path::{Path,PathBuf}`
  - uses: `std::process::{Child,Command,Stdio}`
  - uses: `std::sync::atomic::{AtomicBool,AtomicU32,AtomicU64,Ordering}`
  - uses: `std::sync::{Arc,Mutex}`
  - uses: `std::thread::JoinHandle`
  - uses: `std::time::{Duration,Instant}`
  - uses: `crate::document::septic::{MouseSample,SepticMeta,SepticSession,SEPSCRR_VERSION}`
  - uses: `crate::recorder::{Frame,RecorderConfig,SyncRecorder}`
  - uses: `pipewireaspw`
  - uses: `pw::{properties::properties,spa}`
  - uses: `spa::pod::Pod`
  - uses: `std::mem`
  - uses: `spa::param::audio::AudioFormat`
  - uses: `std::os::fd::AsRawFd`
  - uses: `device_query::DeviceQuery`
  - uses: `pipewire::spa::param::video::VideoFormat`
  - uses: `pipewireaspw`
  - uses: `pw::{properties::properties,spa}`
  - uses: `spa::pod::Pod`

### `src/selection.rs`

- **function** `selection_bounds` (L17)
- **function** `selection_bounds_for_raster` (L37)
- **function** `selected_layer_kind` (L63)
- **function** `selection_is_single_image` (L80)
- **function** `selection_path_and_objects` (L92)
- **function** `selection_path_and_object` (L121)
- **function** `is_tiling_circular_source` (L130)
- **function** `selection_tiling_circular_sources` (L140)
- **function** `selection_has_tiling_effect` (L157)
- **function** `selection_has_circular_effect` (L168)
- **function** `object_on_path_panel_context` (L180)
- **function** `selection_has_object_on_path_effect` (L217)
- **module** `tests` (L228)
- **method** `empty_selection_has_no_bounds` (L233)
- **method** `unknown_ids_are_ignored` (L242)
  - uses: `crate::document::{LayerKind,Node,NodeId,ProjectFile}`
  - uses: `crate::document::NodeKind`
  - uses: `crate::document::{has_effect_for_objects,path_effect_by_form_node}`
  - uses: `super::*`
  - uses: `crate::document::Document`

### `src/shading/cpu_effects.rs`

- **function** `draw_shading_passes` (L8)
- **function** `draw_shading_passes_cpu` (L25)
- **function** `draw_starfield_shader` (L65)
- **function** `draw_galaxy_shader` (L101)
- **function** `draw_blackhole_shader` (L142)
- **function** `append_quad` (L180)
- **function** `draw_vignette` (L207)
- **function** `draw_crt` (L223)
- **function** `append_ring` (L238)
  - uses: `egui::{Color32,Mesh,Painter,Pos2,Rect,Shape,Stroke}`
  - uses: `crate::document::ShadingPass`
  - uses: `crate::shading::procedural_blackhole::{BlackholeParams,sample}`

### `src/shading/cpu_hex_export.rs`

- **function** `hash21` (L8)
- **function** `hex_dist` (L14)
- **function** `hex_gv` (L22)
- **function** `smoothstep` (L39)
- **function** `is_hex_chain_wgsl` (L45)
- **function** `fill_hex_chain_rgba` (L55)
- **function** `fill_hex_chain_rgba_export` (L123)
- **function** `try_fill_pixmap_hex` (L134)
  - uses: `crate::document::ShadingPass`

### `src/shading/graph_blur.rs`

- **class** `GraphBlurEngine` (L69)
- **function** `engine_slot` (L79)
- **method** `create` (L84)
- **method** `with_engine` (L194)
- **method** `blur_to_texture` (L206)
- **method** `blur_inner` (L226)
- **function** `register_or_update_native` (L419)
- **function** `free_native_texture` (L439)
- **function** `gpu_gaussian_blur` (L445)
- **function** `bytemuck_bytes` (L511)
  - uses: `egui_wgpu::wgpu`
  - uses: `image::RgbaImage`
  - uses: `std::sync::OnceLock`

### `src/shading/mod.rs`

- **module** `cpu_effects` (L3)
- **module** `cpu_hex_export` (L4)
- **module** `graph_blur` (L5)
- **module** `procedural_blackhole` (L6)
- **module** `wgpu_pass` (L7)
- **function** `load_wgsl_file` (L18)
- **function** `save_wgsl_file` (L28)
  - uses: `cpu_effects::draw_shading_passes`

### `src/shading/procedural_blackhole.rs`

- **class** `BlackholeParams` (L3)
- **method** `default` (L12)
- **function** `hash21` (L22)
- **function** `smoothstep` (L27)
- **function** `aspect_pos` (L33)
- **function** `lens_uv` (L37)
- **function** `stars` (L48)
- **function** `disk_color` (L67)
- **function** `sample` (L86)
- **function** `sample_starfield` (L142)
- **function** `noise2` (L153)
- **function** `fbm2` (L167)
- **function** `sample_galaxy` (L180)

### `src/shading/wgpu_pass.rs`

- **class** `ShadingGpuResources` (L38)
- **class** `CompiledShadingPipeline` (L50)
- **function** `init_callback_resources` (L56)
- **function** `queue_shading_input` (L69)
- **function** `source_key` (L77)
- **function** `wgsl_needs_compose` (L84)
- **function** `assemble_module` (L88)
- **function** `fragment_entry` (L96)
- **function** `validate_shading_wgsl` (L105)
- **module** `validate_tests` (L177)
- **method** `rejects_glsl_mod` (L181)
- **method** `accepts_fragment_main` (L194)
- **function** `probe_compile_shading_wgsl` (L211)
- **function** `compile_pipeline` (L243)
- **method** `new` (L365)
- **method** `pipeline` (L406)
- **method** `upload_input` (L422)
- **method** `write_uniforms` (L467)
- **method** `bind_group` (L474)
- **function** `uniform_floats` (L507)
- **class** `ShadingPaintCallback` (L521)
- **method** `prepare` (L527)
- **method** `paint` (L546)
- **class** `ShadingRenderer` (L620)
- **method** `default` (L625)
- **method** `new` (L631)
- **function** `is_cpu_only_pass` (L636)
- **function** `active_shading_pass` (L643)
- **function** `try_draw_shading_passes_gpu` (L647)
- **function** `shading_passes_need_input` (L711)
- **function** `offscreen_pipeline` (L721)
- **class** `OffscreenShadePool` (L748)
- **method** `ensure` (L759)
- **function** `render_shading_pass_to_rgba` (L821)
- **function** `composite_shading_layers_into_rgba` (L942)
  - uses: `std::collections::HashMap`
  - uses: `std::sync::Arc`
  - uses: `egui::{Painter,Rect,Shape}`
  - uses: `egui_wgpu::wgpu`
  - uses: `egui::epaint::PaintCallbackInfo`
  - uses: `egui_wgpu::{Callback,CallbackResources,CallbackTrait,RenderState,ScreenDescriptor}`
  - uses: `rustc_hash::FxHasher`
  - uses: `std::hash::{Hash,Hasher}`
  - uses: `crate::document::ShadingPass`
  - uses: `super::validate_shading_wgsl`
  - uses: `std::sync::{Mutex,OnceLock}`
  - uses: `std::sync::{Mutex,OnceLock}`

### `src/spatial_index.rs`

- **class** `SpatialIndex` (L14)
- **method** `disabled` (L23)
- **method** `is_enabled` (L31)
- **method** `rebuild` (L35)
- **method** `pick_topmost_with_document` (L87)
- **method** `candidates_near` (L156)
- **method** `flat_order` (L177)
- **method** `nodes_in_marquee` (L182)
- **function** `cells_for_bounds` (L229)
  - uses: `std::collections::{HashMap,HashSet}`
  - uses: `kurbo::Shape`
  - uses: `rayon::prelude::*`
  - uses: `crate::document::{NodeId,NodeKind,ProjectFile}`

### `src/state.rs`

- **class** `PlaybackState` (L21)
- **method** `default` (L40)
- **method** `time_secs` (L55)
- **module** `tests` (L61)
- **method** `default_playhead_at_zero_60fps` (L65)
  - uses: `super::*`

### `src/sys_stats.rs`

- **class** `SysStats` (L5)
- **method** `default` (L16)
- **method** `new` (L22)
- **method** `update` (L36)
- **class** `JokePlatform` (L170)
- **class** `JokeRule` (L180)
- **function** `parse_jokes` (L205)
- **function** `evaluate_condition` (L272)
- **function** `evaluate_range` (L297)
- **function** `choose_joke` (L337)
- **module** `tests` (L424)
- **method** `joke_cycling_sequence_demo` (L428)
- **method** `evaluate_range_bounds_work` (L465)
  - uses: `std::fs`
  - uses: `std::time::{Instant,Duration}`
  - uses: `std::collections::BTreeMap`
  - uses: `super::*`

### `src/text_glyph.rs`

- **class** `GlyphOutline` (L17)
- **method** `map` (L26)
- **method** `move_to` (L35)
- **method** `line_to` (L43)
- **method** `quad_to` (L47)
- **method** `curve_to` (L52)
- **method** `close` (L60)
- **function** `to_lyon_join` (L66)
- **function** `to_lyon_cap` (L74)
- **function** `screen_norm` (L81)
- **function** `tessellate_fill_mesh` (L87)
- **function** `tessellate_stroke_mesh` (L129)
- **class** `TextCacheKey` (L173)
- **class** `CachedText` (L193)
- **function** `build_text_path_relative` (L203)
- **function** `draw_text_glyphs` (L288)
  - uses: `egui::{Mesh,Painter,Pos2,Shape}`
  - uses: `lyon::math::Point`
  - uses: `lyon::path::Path`
  - uses: `ttf_parser::{Face,GlyphId,OutlineBuilder}`
  - uses: `crate::canvas::Viewport`
  - uses: `crate::document::{Fill,LineCap,LineJoin,TextStyle}`
  - uses: `crate::fonts::FontRegistry`
  - uses: `crate::render::sample_paint_fill`

### `src/theme.rs`

- **module** `colors` (L9)
- **function** `apply` (L41)
- **function** `load_nerd_font` (L111)
- **function** `accent_button` (L131)
- **function** `section_heading` (L156)
- **function** `panel_frame` (L167)
- **function** `chrome_gap` (L171)
- **function** `layout_slot_frame` (L176)
- **function** `floating_card_frame` (L181)
- **function** `show_overlay_area` (L191)
- **function** `show_action_bar_area` (L216)
- **function** `show_floating_panel_area` (L242)
- **function** `show_bottom_slide_panel` (L267)
- **function** `floater_work_rect` (L297)
- **function** `above_status_clip_rect` (L309)
- **function** `bottom_floater_slide_rect` (L316)
- **function** `show_overlay_area_inner` (L334)
- **function** `overlay_work_rect` (L374)
- **function** `bar_frame` (L383)
- **function** `canvas_frame` (L393)
- **function** `action_tab_track_frame` (L403)
- **function** `action_tab_chip` (L418)
- **function** `action_content_frame` (L453)
- **function** `action_content_frame_alpha` (L457)
- **function** `constraint_block` (L468)
- **function** `text_on_background` (L484)
- **function** `paint_status_chip` (L496)
- **function** `paint_status_separator` (L507)
- **function** `status_label_first_line` (L518)
- **function** `measure_status_label` (L529)
- **function** `paint_sliding_label` (L540)
- **function** `paint_powerline_status` (L591)
  - uses: `crate::icons`
  - uses: `egui::Color32`

### `src/tools/mod.rs`

- **module** `weight_flow` (L7)
- **class** `ToolKind` (L13)
- **method** `label` (L41)
- **method** `shortcut` (L64)
- **method** `is_shape_drag` (L88)
- **class** `BrushType` (L103)
- **class** `BrushInputMode` (L114)
- **class** `BrushSession` (L121)
- **method** `default` (L174)
- **function** `pixel_stamp_at` (L217)
- **function** `pixel_cell_index` (L238)
- **function** `pixel_stamps_along` (L249)
- **class** `DragNewShape` (L286)
- **class** `PenSession` (L293)
- **method** `is_empty` (L311)
- **method** `len` (L315)
- **method** `pop_anchor` (L319)
- **method** `to_path_data` (L331)
- **class** `ResizeHandle` (L343)
- **class** `SelectDrag` (L355)
- **class** `MarqueeSelect` (L366)
- **class** `BulkDrag` (L374)
- **class** `SelectSession` (L382)
- **class** `PaintMaskTool` (L414)
- **class** `RasterSelectMode` (L425)
- **class** `RasterSelectSession` (L436)
- **method** `default` (L454)
- **class** `StickyPixelMask` (L474)
- **method** `from_full_frame` (L492)
- **method** `from_region` (L550)
- **method** `contains` (L574)
- **method** `or_with` (L584)
- **method** `invert` (L652)
- **method** `pad_region` (L660)
- **method** `compact` (L692)
- **method** `count_on` (L739)
- **method** `on_bbox` (L744)
- **class** `FloatingPixels` (L774)
- **class** `RasterSession` (L799)
- **method** `default` (L890)
- **class** `RasterBrushPreset` (L946)
- **method** `apply` (L1047)
- **method** `index_of_name` (L1059)
- **class** `ToolState` (L1065)
- **method** `clear_path_point_selection` (L1082)
- **method** `set_single_path_point` (L1087)
- **method** `set_path_segment` (L1092)
- **method** `toggle_path_point` (L1097)
- **method** `primary_path_point` (L1118)
- **method** `points_on_path` (L1122)
- **method** `is_path_point_selected` (L1130)
- **method** `handle_shortcuts` (L1138)
- **function** `doc_point_from_screen` (L1183)
- **function** `screen_from_doc` (L1194)
- **function** `snap_angle_15deg` (L1208)
- **class** `ToolAction` (L1227)
- **method** `default` (L1236)
- **function** `resize_bounds` (L1247)
- **function** `normalize_rect` (L1324)
- **function** `marquee_rect` (L1332)
- **function** `marquee_is_drag` (L1337)
- **function** `shape_size_ok` (L1347)
- **function** `shape_side_ok` (L1352)
- **function** `node_bounds_intersects_marquee` (L1356)
- **module** `pixel_brush_tests` (L1363)
- **method** `hold_still_does_not_spawn_extra_stamps` (L1367)
- **method** `multi_cell_stamp_center_is_not_same_cell_as_pointer` (L1389)
  - uses: `std::collections::HashMap`
  - uses: `egui::{Key,Pos2,Ui,Vec2}`
  - uses: `crate::document::{Node,NodeId,PathData,PathEditTarget,FillKind,GradientStop,Paint}`
  - uses: `super::*`

### `src/tools/weight_flow.rs`

- **class** `WeightFlowMode` (L7)
- **method** `label` (L16)
- **class** `Falloff` (L27)
- **method** `label` (L35)
- **class** `MagneticPole` (L45)
- **method** `label` (L52)
- **class** `WeightFlowConfig` (L61)
- **method** `default` (L76)
- **class** `WeightFlowStroke` (L94)
- **class** `WeightFlowBrush` (L104)
- **method** `is_active` (L114)
- **method** `cancel_stroke` (L118)
  - uses: `crate::document::NodeId`
  - uses: `crate::path_physics::PathPhysicsSim`

### `src/ui.rs`

- **class** `ActionTab` (L22)
- **method** `collab_slug` (L39)
- **method** `from_collab_slug` (L53)
- **method** `all_tabs` (L68)
- **method** `label` (L82)
- **method** `strip_label` (L97)
- **method** `visible_in_strip` (L106)
- **method** `icon` (L129)
- **function** `status_coords_text` (L146)
- **function** `chrome` (L159)
- **function** `menubar_action_toggle` (L228)
- **function** `menubar` (L238)
- **function** `toolbar_tip_layout_job` (L572)
- **function** `show_toolbar_hover_tip` (L659)
- **function** `floating_toolbar` (L684)
- **function** `promote_action_tab` (L1265)
- **function** `promote_action_tab_at` (L1269)
- **function** `select_action_tab_from_strip` (L1284)
- **function** `action_tab_strip` (L1307)
- **function** `action_bar_interior` (L1352)
- **function** `path_magic_section` (L1391)
- **function** `path_magic_card` (L1822)
- **function** `clamp_object_label` (L1836)
- **function** `node_display_name` (L1846)
- **function** `boolean_and_clip_panel` (L1861)
- **function** `object_on_path_container` (L2157)
- **function** `object_on_path_object_label` (L2212)
- **function** `object_on_path_controls` (L2224)
- **function** `floating_action_bar` (L2308)
- **function** `export_section` (L2327)
- **function** `dialog_escape_close` (L2612)
- **function** `hit_pick_menu_overlay` (L2619)
- **function** `plotter_formula_dialog` (L2701)
- **function** `object_rename_dialog` (L2815)
- **function** `daw_piano_dialog` (L2913)
- **function** `video_export_progress_window` (L2940)
- **function** `shader_editor_window` (L3125)
- **function** `shading_wgsl_file_buttons` (L3297)
- **function** `restore_floater_width` (L3381)
- **function** `restore_floater_height` (L3389)
- **function** `floating_video_editor` (L3393)
- **function** `video_audio_extracting` (L3484)
- **function** `video_editor_panel_height` (L3491)
- **function** `best_video_extract_progress` (L3503)
- **function** `video_editor_interior` (L3519)
- **function** `paint_video_editor_extract_banner` (L4316)
- **function** `paint_extract_progress_in_rect` (L4339)
- **function** `status_bar_layout_reserve` (L4371)
- **function** `status_bar_overlay` (L4385)
- **function** `status_bar_body` (L4409)
- **function** `page_section` (L4572)
- **function** `truncate_path_display` (L4691)
- **function** `track_row_with_hover_delete` (L4705)
- **function** `safe_trunc_label` (L4762)
- **function** `layers_section` (L4794)
- **class** `NeGraphRow` (L5716)
- **function** `objects_section` (L5729)
- **function** `ne_output_proxy_inspector` (L6161)
- **function** `appearance_section` (L6319)
- **function** `path_markers_geometry_ui` (L6820)
- **function** `marker_column` (L6882)
- **function** `draw_marker_preview` (L6956)
- **function** `brush_numeric_row` (L7090)
- **function** `paint_brush_tip_preview` (L7108)
- **function** `paint_section` (L7176)
- **function** `geometry_section` (L7433)
- **function** `path_point_bezier_panel` (L8841)
- **function** `show_on_page_text_editor` (L8967)
- **function** `text_style_panel` (L9119)
- **function** `weight_flow_geometry_panel` (L9234)
- **function** `node_icon` (L9372)
- **function** `constraint_origin` (L9401)
- **function** `decimal_drag` (L9418)
- **function** `draw_stylus_3d_preview` (L9436)
- **function** `draw_3d_pen_tip` (L9531)
- **function** `draw_3d_calligraphy_nib` (L9646)
- **class** `TrackPlotInfo` (L9771)
- **function** `draw_timeline_track` (L9778)
- **function** `timeline_interior` (L10139)
- **function** `floating_timeline_window` (L10891)
- **function** `draw_dotted_line` (L11022)
- **function** `stack_region_fill` (L11039)
- **function** `stack_region_border` (L11042)
- **function** `stack_resize_hi` (L11045)
- **function** `graph_editor_interior` (L11049)
- **function** `graph_stack_header_controls` (L12372)
- **function** `apply_stack_animation_function` (L12560)
- **function** `delete_stack_animation_function` (L12639)
- **function** `graph_stack_formula_dialog` (L12662)
- **function** `animation_node_editor_params` (L12774)
- **function** `animation_section` (L12924)
  - uses: `egui::{scroll_area::ScrollBarVisibility,Context,Rect,RichText,ScrollArea,Ui}`
  - uses: `crate::animation::action_bar_overlay_rect`
  - uses: `crate::app::VadadeeBerryApp`
  - uses: `crate::audio_extract::AudioExtractStatus`
  - uses: `crate::document::KeyframeTrack`
  - uses: `crate::icons::{self,nerd_font_id}`
  - uses: `crate::io`
  - uses: `crate::theme::{self,colors}`
  - uses: `crate::tools::ToolKind`
  - uses: `egui::text::{LayoutJob,TextFormat}`
  - uses: `crate::document::CircularRotateMode`
  - uses: `crate::document::BooleanPairMode`
  - uses: `crate::document::BooleanOpKind`
  - uses: `crate::tools::RasterSelectModeasM`
  - uses: `crate::document::BezierHandleMode`
  - uses: `crate::tools::{Falloff,MagneticPole,WeightFlowMode}`

### `src/video_decode.rs`

- **function** `libav_guard` (L18)
- **class** `AVFormatContext` (L31)
- **class** `AVCodecContext` (L32)
- **class** `AVCodec` (L33)
- **class** `AVPacket` (L34)
- **class** `AVFrame` (L35)
- **class** `SwsContext` (L36)
- **class** `AVIOContext` (L37)
- **class** `AVRational` (L41)
- **class** `FfmpegLibs` (L47)
- **function** `try_load_ffmpeg` (L120)
- **macro** `open_lib` (L121)
- **macro** `sym` (L133)
- **function** `stream_time_base_num` (L215)
- **function** `stream_time_base_den` (L218)
- **function** `fmt_stream` (L222)
- **function** `stream_codecpar` (L228)
- **function** `codecpar_width` (L234)
- **function** `codecpar_height` (L235)
- **function** `stream_tb_num` (L236)
- **function** `stream_tb_den` (L242)
- **function** `pkt_stream_index` (L248)
- **function** `frame_data` (L251)
- **function** `frame_linesize` (L254)
- **function** `frame_width` (L257)
- **function** `frame_height` (L258)
- **function** `frame_format` (L259)
- **function** `frame_nb_samples` (L260)
- **function** `codecpar_codec_id` (L263)
- **function** `rgba_aligned_stride` (L270)
- **function** `frame_to_rgba8_packed` (L278)
- **function** `stream_set_time_base` (L388)
- **function** `pkt_set_stream_index` (L400)
- **function** `pkt_set_pts` (L403)
- **function** `pkt_set_dts` (L406)
- **function** `pkt_set_duration` (L409)
- **function** `fmt_nb_streams` (L413)
- **function** `fmt_set_pb` (L417)
- **class** `CodecCtxVideoLayout` (L426)
- **class** `CodecCtxAudioLayout` (L436)
- **function** `codec_ctx_ch_layout_offset` (L447)
- **function** `codec_ctx_audio_layout` (L456)
- **function** `codec_ctx_channels` (L472)
- **function** `frame_ch_layout_offset` (L487)
- **function** `stereo_ch_layout_write_raw` (L492)
- **function** `stereo_ch_layout_apply` (L501)
- **function** `stereo_layout_buf` (L511)
- **function** `codec_ctx_apply_stereo_ch_layout` (L519)
- **function** `frame_prepare_stereo_audio` (L532)
- **function** `codec_ctx_apply_audio_encoder` (L555)
- **function** `codec_ctx_video_layout` (L580)
- **function** `codec_ctx_read_time_base` (L605)
- **function** `stream_read_time_base` (L616)
- **function** `libav_opt_set` (L630)
- **function** `libav_opt_set_int` (L649)
- **function** `libav_opt_get_int` (L667)
- **function** `codec_ctx_read_dims` (L685)
- **function** `encoder_yuv420p_pix_fmt` (L696)
- **function** `codec_ctx_set_pix_fmt` (L707)
- **function** `codec_ctx_read_pix_fmt` (L717)
- **function** `codec_ctx_pix_fmt_ok` (L727)
- **function** `codec_ctx_apply_video` (L732)
- **function** `codec_ctx_dims_ok` (L753)
- **function** `configure_video_encoder` (L758)
- **function** `fmt_duration_secs` (L804)
- **function** `probe_media_size` (L817)
- **function** `probe_media_size_uncached` (L837)
- **function** `probe_media_duration_secs` (L852)
- **function** `decode_frame_cached` (L874)
- **function** `probe_media_duration_libav` (L898)
- **function** `decode_frame` (L944)
- **function** `is_libav_available` (L961)
- **function** `decode_audio_to_mono_f32_libav` (L966)
- **function** `decode_audio_to_stereo_i16_libav` (L1077)
- **function** `write_stereo_i16_as_mp3_libav` (L1211)
- **function** `write_stereo_i16_as_aac_mp4_libav` (L1432)
- **function** `remux_video_and_audio_libav` (L1698)
- **function** `frame_set_nb_samples` (L1882)
- **function** `frame_set_sample_rate` (L1886)
- **function** `frame_set_format` (L1889)
- **function** `frame_set_pts` (L1892)
- **function** `stream_duration` (L1899)
- **function** `append_libav_audio_frame_stereo_i16` (L1906)
- **function** `append_libav_audio_frame` (L1974)
- **function** `decode_libav` (L2050)
- **class** `VideoStream` (L2148)
- **method** `open` (L2176)
- **method** `frame_pts_sec` (L2257)
- **method** `frame_to_rgba_cached` (L2274)
- **method** `get_frame` (L2394)
- **method** `drop` (L2504)
- **class** `LibavEncoder` (L2527)
- **method** `new` (L2551)
- **method** `mux_encoded_packet` (L2801)
- **method** `write_frame` (L2813)
- **method** `finish` (L2878)
- **method** `release_resources` (L2909)
- **method** `drop` (L2943)
- **method** `fmt` (L2953)
- **module** `libav_encoder_tests` (L2963)
- **method** `require_libav` (L2966)
- **method** `libx264_encoder_opens_and_finishes_without_frames` (L2976)
- **method** `libx264_encoder_smoke_writes_mp4` (L2992)
- **method** `configure_video_encoder_sets_yuv420p_pix_fmt` (L3044)
- **method** `aac_sidecar_encoder_opens` (L3072)
- **module** `decode_edge_tests` (L3133)
- **method** `stress_decode_many_frames` (L3136)
- **method** `dump_decoded_frame_right_edge` (L3159)
  - uses: `std::ffi::CString`
  - uses: `std::os::raw::{c_char,c_int}`
  - uses: `std::sync::{Mutex,OnceLock}`
  - uses: `std::collections::HashMap`
  - uses: `std::sync::{Mutex,OnceLock}`
  - uses: `std::collections::HashMap`
  - uses: `std::sync::{Mutex,OnceLock}`
  - uses: `std::sync::Mutex`
  - uses: `super::*`
  - uses: `super::*`

## Top-level crates used

| crate | files |
|---|---|
| `node_graph` | 1 |
| `rand` | 1 |
| `pw` | 2 |
| `glam` | 2 |
| `protocol` | 1 |
| `sync_project` | 1 |
| `sync` | 1 |
| `rayon` | 2 |
| `sha2` | 2 |
| `thiserror` | 1 |
| `tokio_tungstenite` | 4 |
| `fontdb` | 1 |
| `shading` | 1 |
| `animation` | 1 |
| `spa` | 3 |
| `async_recorder` | 1 |
| `aes_gcm` | 2 |
| `node` | 1 |
| `path_effects` | 1 |
| `cpu_effects` | 1 |
| `undo` | 1 |
| `device_query` | 2 |
| `style` | 1 |
| `GraphNodeKind` | 2 |
| `serde` | 18 |
| `rustc_hash` | 2 |
| `desktop` | 1 |
| `PortType` | 2 |
| `indexmap` | 4 |
| `MarkerKind` | 1 |
| `std` | 103 |
| `tokio` | 3 |
| `eframe` | 1 |
| `app` | 1 |
| `uuid` | 15 |
| `crate` | 134 |
| `kurbo` | 16 |
| `async_bridge` | 1 |
| `music` | 1 |
| `symphonia` | 6 |
| `septic` | 1 |
| `kramaframe` | 3 |
| `super` | 37 |
| `rodio` | 2 |
| `pipewire` | 1 |
| `lyon` | 4 |
| `BlendMode` | 1 |
| `egui` | 20 |
| `stub` | 1 |
| `base64` | 7 |
| `FlowchartEdgeSide` | 1 |
| `pipewireaspw` | 2 |
| `usvg` | 1 |
| `opencv` | 4 |
| `expr` | 1 |
| `serde_json` | 3 |
| `geo` | 6 |
| `PortDir` | 2 |
| `av_clip` | 1 |
| `futures_util` | 2 |
| `ttf_parser` | 1 |
| `egui_wgpu` | 4 |
| `image` | 8 |
| `resvg` | 4 |

## Dependency graph (mermaid)

```mermaid
flowchart TD
  subgraph icons[icons]
    icons["icons"]
  end
  subgraph theme[theme]
    theme["theme"]
  end
  subgraph selection[selection]
    selection["selection"]
  end
  subgraph canvas[canvas]
    canvas["canvas"]
  end
  subgraph main[main]
    main["main"]
  end
  subgraph fonts[fonts]
    fonts["fonts"]
  end
  subgraph mcp[mcp]
    mcp__drawing["mcp::drawing"]
    mcp["mcp"]
    mcp__node_editor["mcp::node_editor"]
    mcp__path_parse["mcp::path_parse"]
  end
  subgraph cv[cv]
    cv__jobs["cv::jobs"]
    cv["cv"]
    cv__opencv_face["cv::opencv_face"]
    cv__ops["cv::ops"]
    cv__track["cv::track"]
  end
  subgraph shading[shading]
    shading__cpu_effects["shading::cpu_effects"]
    shading__cpu_hex_export["shading::cpu_hex_export"]
    shading__graph_blur["shading::graph_blur"]
    shading["shading"]
    shading__procedural_blackhole["shading::procedural_blackhole"]
    shading__wgpu_pass["shading::wgpu_pass"]
  end
  subgraph animation[animation]
    animation["animation"]
  end
  subgraph state[state]
    state["state"]
  end
  subgraph perf[perf]
    perf["perf"]
  end
  subgraph node_editor_ui[node_editor_ui]
    node_editor_ui["node_editor_ui"]
  end
  subgraph io[io]
    io["io"]
  end
  subgraph layer_cache[layer_cache]
    layer_cache["layer_cache"]
  end
  subgraph audio_extract[audio_extract]
    audio_extract["audio_extract"]
  end
  subgraph app[app]
    app["app"]
  end
  subgraph video_decode[video_decode]
    video_decode["video_decode"]
  end
  subgraph document[document]
    document__animation["document::animation"]
    document__av_clip["document::av_clip"]
    document__expr["document::expr"]
    document__flowchart["document::flowchart"]
    document["document"]
    document__music["document::music"]
    document__node_graph["document::node_graph"]
    document__node["document::node"]
    document__path_effects["document::path_effects"]
    document__septic["document::septic"]
    document__shading["document::shading"]
    document__style["document::style"]
  end
  subgraph commands[commands]
    commands["commands"]
  end
  subgraph text_glyph[text_glyph]
    text_glyph["text_glyph"]
  end
  subgraph render[render]
    render["render"]
  end
  subgraph export_audio[export_audio]
    export_audio["export_audio"]
  end
  subgraph ui[ui]
    ui["ui"]
  end
  subgraph recorder[recorder]
    recorder__async_bridge["recorder::async_bridge"]
    recorder__async_recorder["recorder::async_recorder"]
    recorder["recorder"]
    recorder__sync["recorder::sync"]
  end
  subgraph left_dock[left_dock]
    left_dock["left_dock"]
  end
  subgraph sys_stats[sys_stats]
    sys_stats["sys_stats"]
  end
  subgraph collab[collab]
    collab__desktop["collab::desktop"]
    collab["collab"]
    collab__protocol["collab::protocol"]
    collab__relay["collab::relay"]
    collab__stub["collab::stub"]
    collab__sync_project["collab::sync_project"]
  end
  subgraph export_worker[export_worker]
    export_worker["export_worker"]
  end
  subgraph history[history]
    history["history"]
  end
  subgraph av_ui[av_ui]
    av_ui["av_ui"]
  end
  subgraph blend[blend]
    blend["blend"]
  end
  subgraph tools[tools]
    tools["tools"]
    tools__weight_flow["tools::weight_flow"]
  end
  subgraph screen_capture[screen_capture]
    screen_capture["screen_capture"]
  end
  subgraph path_physics[path_physics]
    path_physics["path_physics"]
  end
  subgraph raster[raster]
    raster["raster"]
  end
  subgraph bin[bin]
    bin__vadadee_mcp_stdio["bin::vadadee_mcp_stdio"]
  end
  subgraph export_types[export_types]
    export_types["export_types"]
  end
  subgraph spatial_index[spatial_index]
    spatial_index["spatial_index"]
  end
  subgraph gradient_ui[gradient_ui]
    gradient_ui["gradient_ui"]
  end
  subgraph lib[lib]
    lib["lib"]
  end
  animation --> tools
  animation --> ui
  animation --> theme
  app --> animation
  app --> canvas
  app --> fonts
  app --> commands
  app --> history
  app --> io
  app --> render
  app --> theme
  app --> tools
  app --> audio_extract
  app --> document
  app --> ui
  app --> export_types
  app --> gradient_ui
  app --> path_physics
  app --> mcp
  app --> mcp__drawing
  av_ui --> app
  av_ui --> document
  av_ui --> icons
  blend --> document
  collab__desktop --> collab__protocol
  collab__stub --> collab__protocol
  collab__sync_project --> document
  commands --> document
  commands --> history
  document__animation --> document
  document__path_effects --> document
  export_audio --> export_types
  export_audio --> document
  export_worker --> export_types
  export_worker --> document
  export_worker --> io
  export_worker --> recorder
  export_worker --> video_decode
  fonts --> icons
  gradient_ui --> theme
  history --> document
  history --> export_types
  io --> document
  layer_cache --> document
  left_dock --> animation
  left_dock --> app
  left_dock --> collab
  left_dock --> icons
  left_dock --> theme
  left_dock --> tools
  mcp__drawing --> document
  mcp__node_editor --> document
  node_editor_ui --> app
  node_editor_ui --> icons
  node_editor_ui --> theme
  path_physics --> tools__weight_flow
  recorder__sync --> video_decode
  render --> canvas
  render --> document
  render --> theme
  render --> gradient_ui
  render --> tools
  screen_capture --> document__septic
  screen_capture --> recorder
  selection --> document
  shading__cpu_effects --> document
  shading__cpu_effects --> shading__procedural_blackhole
  shading__cpu_hex_export --> document
  shading__wgpu_pass --> document
  spatial_index --> document
  text_glyph --> canvas
  text_glyph --> document
  text_glyph --> fonts
  text_glyph --> render
  theme --> icons
  tools --> document
  tools__weight_flow --> document
  tools__weight_flow --> path_physics
  ui --> animation
  ui --> app
  ui --> audio_extract
  ui --> document
  ui --> icons
  ui --> io
  ui --> theme
  ui --> tools
```
