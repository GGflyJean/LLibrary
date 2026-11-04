use anyhow::Result;

use crate::db::Db;

#[derive(Debug, Clone, Copy)]
pub struct Level1 {
    pub code: char,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Category {
    pub code: &'static str,
    pub label: &'static str,
    pub keywords: &'static [&'static str],
}

pub const LEVEL1: &[Level1] = &[
    Level1 {
        code: 'A',
        label: "马克思主义、列宁主义、毛泽东思想、邓小平理论",
    },
    Level1 {
        code: 'B',
        label: "哲学、宗教",
    },
    Level1 {
        code: 'C',
        label: "社会科学总论",
    },
    Level1 {
        code: 'D',
        label: "政治、法律",
    },
    Level1 {
        code: 'E',
        label: "军事",
    },
    Level1 {
        code: 'F',
        label: "经济",
    },
    Level1 {
        code: 'G',
        label: "文化、科学、教育、体育",
    },
    Level1 {
        code: 'H',
        label: "语言、文字",
    },
    Level1 {
        code: 'I',
        label: "文学",
    },
    Level1 {
        code: 'J',
        label: "艺术",
    },
    Level1 {
        code: 'K',
        label: "历史、地理",
    },
    Level1 {
        code: 'N',
        label: "自然科学总论",
    },
    Level1 {
        code: 'O',
        label: "数理科学和化学",
    },
    Level1 {
        code: 'P',
        label: "天文学、地球科学",
    },
    Level1 {
        code: 'Q',
        label: "生物科学",
    },
    Level1 {
        code: 'R',
        label: "医药、卫生",
    },
    Level1 {
        code: 'S',
        label: "农业科学",
    },
    Level1 {
        code: 'T',
        label: "工业技术",
    },
    Level1 {
        code: 'U',
        label: "交通运输",
    },
    Level1 {
        code: 'V',
        label: "航空、航天",
    },
    Level1 {
        code: 'X',
        label: "环境科学、安全科学",
    },
    Level1 {
        code: 'Z',
        label: "综合性图书",
    },
];

pub const CATEGORIES: &[Category] = &[
    Category {
        code: "A7",
        label: "马克思主义理论",
        keywords: &[
            "马克思",
            "恩格斯",
            "列宁",
            "毛泽东",
            "邓小平",
            "资本论",
            "社会主义",
            "共产主义",
            "唯物史观",
            "共产党宣言",
        ],
    },
    Category {
        code: "B0",
        label: "哲学理论",
        keywords: &[
            "哲学理论",
            "哲学导论",
            "哲学是什么",
            "哲学问题",
            "形而上学",
            "认识论",
        ],
    },
    Category {
        code: "B1",
        label: "世界哲学",
        keywords: &["世界哲学", "哲学史", "哲学通史"],
    },
    Category {
        code: "B2",
        label: "中国哲学",
        keywords: &[
            "中国哲学",
            "儒家",
            "道家",
            "论语",
            "孟子",
            "老子",
            "庄子",
            "荀子",
            "韩非",
            "墨子",
            "朱熹",
            "王阳明",
            "心学",
            "理学",
            "经学",
            "玄学",
            "孔子",
            "大学",
            "中庸",
            "周易",
        ],
    },
    Category {
        code: "B3",
        label: "亚洲哲学",
        keywords: &["印度哲学", "日本哲学", "亚洲哲学"],
    },
    Category {
        code: "B5",
        label: "欧洲哲学",
        keywords: &[
            "西方哲学",
            "康德",
            "黑格尔",
            "尼采",
            "海德格尔",
            "柏拉图",
            "亚里士多德",
            "笛卡尔",
            "休谟",
            "福柯",
            "德里达",
            "哈贝马斯",
            "存在主义",
            "现象学",
            "分析哲学",
        ],
    },
    Category {
        code: "B7",
        label: "美洲哲学",
        keywords: &["美国哲学", "美洲哲学"],
    },
    Category {
        code: "B81",
        label: "逻辑学",
        keywords: &["逻辑学", "逻辑"],
    },
    Category {
        code: "B82",
        label: "伦理学",
        keywords: &["伦理学", "伦理", "道德", "道德哲学"],
    },
    Category {
        code: "B83",
        label: "美学",
        keywords: &["美学", "审美", "艺术哲学", "美的历程"],
    },
    Category {
        code: "B84",
        label: "心理学",
        keywords: &["心理学", "精神分析", "心理", "弗洛伊德", "荣格"],
    },
    Category {
        code: "B9",
        label: "宗教",
        keywords: &[
            "宗教",
            "佛教",
            "禅宗",
            "基督教",
            "圣经",
            "伊斯兰",
            "道教",
            "神学",
        ],
    },
    Category {
        code: "C0",
        label: "社会科学总论",
        keywords: &["社会科学", "社会科学总论", "社会科学理论"],
    },
    Category {
        code: "C8",
        label: "统计学",
        keywords: &["统计学", "统计"],
    },
    Category {
        code: "C91",
        label: "社会学",
        keywords: &["社会学", "社会理论", "社会学家", "社会"],
    },
    Category {
        code: "C92",
        label: "人口学",
        keywords: &["人口学", "人口"],
    },
    Category {
        code: "C93",
        label: "管理学",
        keywords: &["管理学", "管理", "组织"],
    },
    Category {
        code: "C95",
        label: "民族学",
        keywords: &["民族学", "民族", "人类学", "族群"],
    },
    Category {
        code: "D0",
        label: "政治理论",
        keywords: &[
            "政治学",
            "政治哲学",
            "政治理论",
            "民主",
            "自由",
            "国家",
            "权力",
            "政治",
        ],
    },
    Category {
        code: "D6",
        label: "中国政治",
        keywords: &["中国政治", "中国政治制度", "政治制度"],
    },
    Category {
        code: "D8",
        label: "外交、国际关系",
        keywords: &["国际关系", "外交", "国际政治", "地缘政治"],
    },
    Category {
        code: "D9",
        label: "法律",
        keywords: &["法律", "法学", "宪法", "法理", "民法", "刑法", "法治"],
    },
    Category {
        code: "E0",
        label: "军事理论",
        keywords: &["军事", "军事理论", "战争", "战略"],
    },
    Category {
        code: "E8",
        label: "战略、战役、战术",
        keywords: &["战役", "战术"],
    },
    Category {
        code: "F0",
        label: "经济学",
        keywords: &[
            "经济学",
            "经济",
            "微观经济",
            "宏观经济",
            "政治经济学",
            "财富",
            "市场",
            "企业家",
            "资本",
            "21世纪资本论",
        ],
    },
    Category {
        code: "F1",
        label: "世界经济",
        keywords: &["世界经济", "全球经济", "国际经济"],
    },
    Category {
        code: "F2",
        label: "中国经济",
        keywords: &["中国经济", "中国经济史", "中国经济思想", "改革开放"],
    },
    Category {
        code: "F7",
        label: "贸易经济",
        keywords: &["贸易", "贸易经济", "商业"],
    },
    Category {
        code: "F8",
        label: "财政、金融",
        keywords: &["金融", "货币", "银行", "投资", "证券", "保险", "财政"],
    },
    Category {
        code: "G0",
        label: "文化理论",
        keywords: &["文化", "文化研究", "文化理论", "大众文化"],
    },
    Category {
        code: "G2",
        label: "信息与知识传播",
        keywords: &["新闻", "传播", "媒体", "出版", "图书馆", "信息"],
    },
    Category {
        code: "G4",
        label: "教育",
        keywords: &["教育", "教育学", "大学", "高等教育"],
    },
    Category {
        code: "H0",
        label: "语言学",
        keywords: &["语言学", "语言", "语法", "语义", "语用"],
    },
    Category {
        code: "H1",
        label: "汉语",
        keywords: &[
            "汉语",
            "中文",
            "古代汉语",
            "现代汉语",
            "音韵",
            "训诂",
            "文字",
            "词汇",
            "词义",
        ],
    },
    Category {
        code: "H3",
        label: "常用外国语",
        keywords: &["英语", "外语", "翻译"],
    },
    Category {
        code: "I0",
        label: "文学理论",
        keywords: &["文学理论", "文学批评", "文艺理论", "叙事学", "诗学"],
    },
    Category {
        code: "I1",
        label: "世界文学",
        keywords: &["世界文学", "世界文学史", "外国文学", "比较文学"],
    },
    Category {
        code: "I2",
        label: "中国文学",
        keywords: &[
            "中国文学",
            "文学",
            "诗",
            "词",
            "曲",
            "小说",
            "散文",
            "戏剧",
            "红楼梦",
            "鲁迅",
            "杜甫",
            "李白",
            "苏轼",
            "曹雪芹",
            "叶嘉莹",
            "词选",
            "诗选",
            "作品集",
            "文集",
            "现代文学",
            "当代文学",
            "古典文学",
            "文学史",
        ],
    },
    Category {
        code: "I3",
        label: "亚洲文学",
        keywords: &["日本文学", "韩国文学", "印度文学", "亚洲文学"],
    },
    Category {
        code: "I5",
        label: "欧洲文学",
        keywords: &[
            "欧洲文学",
            "俄国文学",
            "俄罗斯文学",
            "英国文学",
            "法国文学",
            "德国文学",
            "西方文学",
        ],
    },
    Category {
        code: "I7",
        label: "美洲文学",
        keywords: &["美国文学", "美洲文学", "拉丁美洲文学"],
    },
    Category {
        code: "J0",
        label: "艺术理论",
        keywords: &["艺术", "艺术理论", "艺术史", "艺术哲学"],
    },
    Category {
        code: "J2",
        label: "绘画",
        keywords: &["绘画", "中国画", "美术", "画论", "美术史"],
    },
    Category {
        code: "J29",
        label: "书法、篆刻",
        keywords: &["书法", "篆刻"],
    },
    Category {
        code: "J6",
        label: "音乐",
        keywords: &["音乐"],
    },
    Category {
        code: "J8",
        label: "戏剧艺术",
        keywords: &["戏剧", "戏曲", "话剧"],
    },
    Category {
        code: "J9",
        label: "电影、电视艺术",
        keywords: &["电影", "影视", "电影史"],
    },
    Category {
        code: "K0",
        label: "史学理论",
        keywords: &["史学", "史学理论", "历史学", "历史哲学"],
    },
    Category {
        code: "K1",
        label: "世界史",
        keywords: &["世界史", "全球史", "世界文明", "人类史", "文明史"],
    },
    Category {
        code: "K2",
        label: "中国史",
        keywords: &[
            "中国史",
            "中国历史",
            "历史",
            "史记",
            "通鉴",
            "汉书",
            "后汉书",
            "三国志",
            "唐书",
            "宋史",
            "明史",
            "清史",
            "春秋",
            "战国",
            "秦汉",
            "魏晋",
            "隋唐",
            "宋元",
            "明清",
            "近代史",
            "现代史",
            "革命",
            "王朝",
            "皇帝",
            "制度",
        ],
    },
    Category {
        code: "K3",
        label: "亚洲史",
        keywords: &["亚洲史", "日本史", "韩国史", "印度史"],
    },
    Category {
        code: "K5",
        label: "欧洲史",
        keywords: &[
            "欧洲史",
            "英国史",
            "法国史",
            "德国史",
            "俄国史",
            "罗马史",
            "中世纪",
        ],
    },
    Category {
        code: "K7",
        label: "美洲史",
        keywords: &["美国史", "美洲史", "拉丁美洲史"],
    },
    Category {
        code: "K81",
        label: "传记",
        keywords: &["传记", "自传", "回忆录", "日记", "人物"],
    },
    Category {
        code: "K85",
        label: "文物考古",
        keywords: &["考古", "文物", "博物馆", "古墓", "敦煌"],
    },
    Category {
        code: "K89",
        label: "风俗习惯",
        keywords: &["风俗", "民俗", "礼仪", "节日"],
    },
    Category {
        code: "K9",
        label: "地理",
        keywords: &["地理", "游记", "地图", "城市"],
    },
    Category {
        code: "N0",
        label: "自然科学总论",
        keywords: &["科学", "科学史", "科学哲学", "自然科学", "科学革命"],
    },
    Category {
        code: "O1",
        label: "数学",
        keywords: &["数学", "几何", "代数", "微积分"],
    },
    Category {
        code: "O4",
        label: "物理学",
        keywords: &["物理", "物理学", "光学", "电磁"],
    },
    Category {
        code: "O6",
        label: "化学",
        keywords: &["化学"],
    },
    Category {
        code: "P1",
        label: "天文学",
        keywords: &["天文", "天文学", "宇宙"],
    },
    Category {
        code: "P5",
        label: "地质学",
        keywords: &["地质", "地质学"],
    },
    Category {
        code: "P9",
        label: "自然地理学",
        keywords: &["自然地理"],
    },
    Category {
        code: "Q1",
        label: "普通生物学",
        keywords: &["生物", "生物学", "进化", "遗传"],
    },
    Category {
        code: "Q3",
        label: "遗传学",
        keywords: &["遗传学", "基因"],
    },
    Category {
        code: "Q94",
        label: "植物学",
        keywords: &["植物", "植物学"],
    },
    Category {
        code: "Q95",
        label: "动物学",
        keywords: &["动物", "动物学"],
    },
    Category {
        code: "Q98",
        label: "人类学",
        keywords: &["人类学", "人类起源"],
    },
    Category {
        code: "R1",
        label: "预防医学、卫生学",
        keywords: &["医学", "健康", "卫生", "预防医学"],
    },
    Category {
        code: "R2",
        label: "中国医学",
        keywords: &["中医", "中国医学"],
    },
    Category {
        code: "R4",
        label: "临床医学",
        keywords: &["临床医学"],
    },
    Category {
        code: "R9",
        label: "药学",
        keywords: &["药学", "药物"],
    },
    Category {
        code: "S1",
        label: "农业基础科学",
        keywords: &["农业", "农学", "作物", "土壤", "植物保护"],
    },
    Category {
        code: "S5",
        label: "农作物",
        keywords: &["农作物", "作物栽培"],
    },
    Category {
        code: "T0",
        label: "工业技术",
        keywords: &[
            "计算机",
            "编程",
            "软件",
            "网络",
            "算法",
            "数据库",
            "Linux",
            "C++",
            "Java",
            "Python",
            "Go",
            "Rust",
            "工程",
            "技术",
        ],
    },
    Category {
        code: "U1",
        label: "综合运输",
        keywords: &["交通", "运输"],
    },
    Category {
        code: "V1",
        label: "航空、航天",
        keywords: &["航空", "航天"],
    },
    Category {
        code: "X1",
        label: "环境科学基础理论",
        keywords: &["环境", "生态", "环保", "环境科学"],
    },
    Category {
        code: "Z1",
        label: "丛书",
        keywords: &["丛书"],
    },
    Category {
        code: "Z2",
        label: "百科全书、类书",
        keywords: &["百科", "百科全书"],
    },
    Category {
        code: "Z3",
        label: "辞典",
        keywords: &["辞典", "词典"],
    },
    Category {
        code: "Z8",
        label: "图书目录、文摘、索引",
        keywords: &["书目", "索引", "文摘"],
    },
];

pub const UNCLASSIFIED_CODE: &str = "-";

pub fn level1_label(code: &str) -> &'static str {
    let first = code.chars().next().unwrap_or('Z');
    LEVEL1
        .iter()
        .find(|level| level.code == first)
        .map(|level| level.label)
        .unwrap_or("综合性图书")
}

pub fn level2_label(code: &str) -> Option<&'static str> {
    CATEGORIES
        .iter()
        .find(|category| category.code == code)
        .map(|category| category.label)
}

pub fn classify(
    title: &str,
    authors: &str,
    publisher: &str,
    description: &str,
) -> Option<(String, String)> {
    let text = format!("{title} {authors} {publisher} {description}");
    let mut best: Option<(&Category, i32)> = None;
    for category in CATEGORIES {
        let mut score = 0;
        for keyword in category.keywords {
            let weight = keyword.chars().count().min(8) as i32;
            if title.contains(keyword) {
                score += 4 + weight;
            } else if text.contains(keyword) {
                score += 1 + weight;
            }
        }
        if score > 0
            && best
                .map(|(_, best_score)| score > best_score)
                .unwrap_or(true)
        {
            best = Some((category, score));
        }
    }
    if let Some((category, _)) = best {
        return Some((category.code.to_string(), category.label.to_string()));
    }

    let code = match publisher.trim() {
        "人民文学出版社" => "I2",
        "中华书局" => "K2",
        "中信出版社" => "F0",
        "北京大学出版社" => "C0",
        _ => return None,
    };
    Some((
        code.to_string(),
        level2_label(code).unwrap_or(code).to_string(),
    ))
}

pub fn classify_unclassified(db: &Db) -> Result<usize> {
    let mut classified = 0;
    loop {
        let batch = db.unclassified_works(500)?;
        if batch.is_empty() {
            break;
        }
        let batch_len = batch.len();
        let mut updates = Vec::with_capacity(batch_len);
        for work in batch {
            match classify(
                &work.title,
                &work.authors,
                &work.publisher,
                &work.description,
            ) {
                Some((code, label)) => {
                    updates.push((work.id, code, label));
                    classified += 1;
                }
                None => {
                    updates.push((work.id, UNCLASSIFIED_CODE.to_string(), "未分类".to_string()));
                }
            }
        }
        db.set_work_clc_batch(&updates)?;
        if batch_len < 500 {
            break;
        }
    }
    Ok(classified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_subjects() {
        assert_eq!(
            classify("论语译注", "杨伯峻", "中华书局", "").map(|value| value.0),
            Some("B2".to_string())
        );
        assert_eq!(
            classify("红楼梦", "曹雪芹", "人民文学出版社", "").map(|value| value.0),
            Some("I2".to_string())
        );
        assert_eq!(
            classify("史记", "司马迁", "中华书局", "").map(|value| value.0),
            Some("K2".to_string())
        );
        assert_eq!(
            classify("经济学原理", "曼昆", "北京大学出版社", "").map(|value| value.0),
            Some("F0".to_string())
        );
        assert_eq!(
            classify("西方哲学史", "罗素", "商务印书馆", "").map(|value| value.0),
            Some("B5".to_string())
        );
        assert_eq!(
            classify("美的历程", "李泽厚", "", "").map(|value| value.0),
            Some("B83".to_string())
        );
        assert_eq!(
            classify("21世纪资本论", "皮凯蒂", "中信出版社", "").map(|value| value.0),
            Some("F0".to_string())
        );
        assert!(classify("完全未知的书", "", "", "").is_none());
        assert_eq!(
            classify("某本小说", "", "人民文学出版社", "").map(|value| value.0),
            Some("I2".to_string())
        );
    }

    #[test]
    fn labels_are_available() {
        assert_eq!(level1_label("I2"), "文学");
        assert_eq!(level2_label("I2"), Some("中国文学"));
        assert_eq!(level2_label("K2"), Some("中国史"));
    }
}
